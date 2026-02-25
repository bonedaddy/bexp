use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

mod capsule;
mod config;
mod db;
mod error;
mod git;
mod graph;
mod indexer;
mod lsp;
mod mcp;
mod memory;
mod skeleton;
mod types;
mod workspace;

use config::VexpConfig;
use db::Database;
use graph::GraphEngine;
use indexer::IndexerService;
use mcp::server::VexpServer;
use memory::MemoryService;
use skeleton::Skeletonizer;

#[derive(Parser)]
#[command(name = "vexp", version, about = "Local-first context engine for AI coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the MCP server over stdio
    Serve {
        /// Workspace root directory (default: current directory)
        #[arg(short, long)]
        workspace: Option<PathBuf>,
    },
    /// Index the workspace
    Index {
        /// Workspace root directory (default: current directory)
        #[arg(short, long)]
        workspace: Option<PathBuf>,
    },
    /// Flush WAL to main database file
    FlushWal {
        #[arg(short, long)]
        workspace: Option<PathBuf>,
    },
    /// Re-index the workspace
    Reindex {
        #[arg(short, long)]
        workspace: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing to stderr (stdout is reserved for MCP JSON-RPC)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { workspace } => {
            let workspace_root = workspace
                .unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            serve(workspace_root).await?;
        }
        Commands::Index { workspace } => {
            let workspace_root = workspace
                .unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            index_workspace(&workspace_root)?;
        }
        Commands::FlushWal { workspace } => {
            let workspace_root = workspace
                .unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            let config = VexpConfig::load(&workspace_root)?;
            let db = Database::open(&config.db_path(&workspace_root))?;
            db.flush_wal()?;
            eprintln!("WAL flushed.");
        }
        Commands::Reindex { workspace } => {
            let workspace_root = workspace
                .unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            index_workspace(&workspace_root)?;
        }
    }

    Ok(())
}

fn index_workspace(workspace_root: &std::path::Path) -> anyhow::Result<()> {
    let config = Arc::new(VexpConfig::load(workspace_root)?);
    let db = Arc::new(Database::open(&config.db_path(workspace_root))?);
    let indexer = IndexerService::new(db.clone(), config.clone(), workspace_root.to_path_buf());

    let report = indexer.full_index()?;
    eprintln!(
        "Indexed: {} files, {} nodes, {} edges",
        report.file_count, report.node_count, report.edge_count
    );

    Ok(())
}

async fn serve(workspace_root: PathBuf) -> anyhow::Result<()> {
    tracing::info!("Starting vexp server for {}", workspace_root.display());

    let config = Arc::new(VexpConfig::load(&workspace_root)?);
    let db = Arc::new(Database::open(&config.db_path(&workspace_root))?);

    // Build services
    let graph = Arc::new(GraphEngine::new());
    let indexer = Arc::new(IndexerService::new(
        db.clone(),
        config.clone(),
        workspace_root.clone(),
    ));

    let skeletonizer = Arc::new(Skeletonizer::new(db.clone()));
    let memory = Arc::new(MemoryService::new(db.clone(), graph.clone()));
    let capsule = Arc::new(capsule::CapsuleGenerator::new(
        db.clone(),
        config.clone(),
        graph.clone(),
        skeletonizer.clone(),
        memory.clone(),
    ));

    // Start file watcher early so changes are captured immediately.
    let _watcher = indexer::watcher::FileWatcher::start(
        workspace_root.clone(),
        config.clone(),
        indexer.clone(),
        graph.clone(),
        db.clone(),
    );
    if _watcher.is_ok() {
        tracing::info!("File watcher started");
    } else {
        tracing::warn!("File watcher failed to start: {:?}", _watcher.err());
    }

    // Run initial index asynchronously so MCP clients can connect immediately.
    // This avoids startup timeouts on large repositories.
    let startup_indexer = indexer.clone();
    let startup_graph = graph.clone();
    let startup_db = db.clone();
    let startup_config = config.clone();
    let startup_workspace = workspace_root.clone();
    tokio::task::spawn_blocking(move || {
        tracing::info!("Running initial index in background...");
        match startup_indexer.full_index() {
            Ok(report) => {
                tracing::info!(
                    "Initial index complete: {} files, {} nodes, {} edges",
                    report.file_count,
                    report.node_count,
                    report.edge_count
                );

                if let Err(e) = startup_graph.rebuild_from_db(&startup_db.reader()) {
                    tracing::error!("Graph rebuild after initial index failed: {}", e);
                } else {
                    tracing::info!(
                        "Graph loaded: {} nodes, {} edges",
                        startup_graph.node_count(),
                        startup_graph.edge_count()
                    );
                }

                // Mark index as ready
                startup_indexer.set_index_ready(true);

                // Run LSP resolution if enabled
                if startup_config.lsp_resolution {
                    match lsp::resolver::resolve_via_lsp(
                        &startup_db,
                        &startup_config,
                        &startup_graph,
                        &startup_workspace,
                    ) {
                        Ok(resolved) => {
                            if resolved > 0 {
                                tracing::info!("LSP resolution: {} edges created", resolved);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("LSP resolution failed: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Initial index failed: {}", e);
                // Mark ready even on failure so tools don't hang forever
                startup_indexer.set_index_ready(true);
            }
        }
    });

    // Create MCP server
    let server = VexpServer::new(
        db.clone(),
        config.clone(),
        indexer.clone(),
        graph.clone(),
        skeletonizer.clone(),
        capsule.clone(),
        memory.clone(),
        workspace_root,
    );

    tracing::info!("Starting MCP stdio server...");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    // Cleanup
    tracing::info!("Server shutting down, flushing WAL...");
    db.flush_wal()?;

    Ok(())
}
