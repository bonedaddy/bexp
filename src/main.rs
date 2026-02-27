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
mod health;
mod indexer;
mod lsp;
mod mcp;
mod memory;
mod metrics;
mod skeleton;
mod types;
mod workspace;

use config::BexpConfig;
use db::Database;
use graph::GraphEngine;
use indexer::IndexerService;
use mcp::server::BexpServer;
use memory::MemoryService;
use skeleton::Skeletonizer;

#[derive(Parser)]
#[command(
    name = "bexp",
    version,
    about = "Local-first context engine for AI coding agents"
)]
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

        /// Port for health/metrics HTTP endpoint (optional)
        #[arg(long)]
        health_port: Option<u16>,
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
        Commands::Serve {
            workspace,
            health_port,
        } => {
            let workspace_root =
                workspace.unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            serve(workspace_root, health_port).await?;
        }
        Commands::Index { workspace } => {
            let workspace_root =
                workspace.unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            index_workspace(&workspace_root)?;
        }
        Commands::FlushWal { workspace } => {
            let workspace_root =
                workspace.unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            let config = BexpConfig::load(&workspace_root)?;
            let db = Database::open(&config.db_path(&workspace_root))?;
            db.flush_wal()?;
            eprintln!("WAL flushed.");
        }
        Commands::Reindex { workspace } => {
            let workspace_root =
                workspace.unwrap_or_else(|| std::env::current_dir().expect("Cannot get cwd"));
            index_workspace(&workspace_root)?;
        }
    }

    Ok(())
}

fn index_workspace(workspace_root: &std::path::Path) -> anyhow::Result<()> {
    // Auto-discover git submodules and add them to workspace_group
    let submodules = git::discover_submodules(workspace_root);

    let mut config = BexpConfig::load(workspace_root)?;
    for sub_path in &submodules {
        let sub_str = sub_path.to_string_lossy().to_string();
        if !config.workspace_group.contains(&sub_str) {
            config.workspace_group.push(sub_str);
        }
    }

    let extra_roots = compute_extra_roots(&config, workspace_root);
    let config = Arc::new(config);
    let db = Arc::new(Database::open(&config.db_path(workspace_root))?);
    let indexer = IndexerService::new(
        db.clone(),
        config.clone(),
        workspace_root.to_path_buf(),
        extra_roots,
    );

    let report = indexer.full_index()?;
    eprintln!(
        "Indexed: {} files, {} nodes, {} edges",
        report.file_count, report.node_count, report.edge_count
    );

    Ok(())
}

/// Compute extra roots from workspace_group, filtering out paths already under workspace_root.
fn compute_extra_roots(config: &BexpConfig, workspace_root: &std::path::Path) -> Vec<PathBuf> {
    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    config
        .workspace_group
        .iter()
        .filter_map(|s| {
            let p = PathBuf::from(s);
            let p = if p.is_relative() {
                workspace_root.join(&p)
            } else {
                p
            };
            let p = p.canonicalize().ok()?;
            if p.starts_with(&canonical_root) {
                None // Already scanned by main WalkDir
            } else if p.is_dir() {
                Some(p)
            } else {
                tracing::warn!(path = %p.display(), "Workspace group path not found or not a directory, skipping");
                None
            }
        })
        .collect()
}

async fn serve(workspace_root: PathBuf, health_port_override: Option<u16>) -> anyhow::Result<()> {
    tracing::info!(workspace = %workspace_root.display(), "Starting bexp server");

    let mut config = BexpConfig::load(&workspace_root)?;
    // CLI --health-port overrides config file
    if health_port_override.is_some() {
        config.health_port = health_port_override;
    }
    // Auto-discover git submodules and add them to workspace_group for unified indexing.
    let submodules = git::discover_submodules(&workspace_root);
    for sub_path in &submodules {
        let sub_str = sub_path.to_string_lossy().to_string();
        if !config.workspace_group.contains(&sub_str) {
            config.workspace_group.push(sub_str);
        }
    }
    let extra_roots = compute_extra_roots(&config, &workspace_root);
    let config = Arc::new(config);

    // Install Prometheus metrics recorder
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let prometheus_handle = recorder.handle();
    let _ = ::metrics::set_global_recorder(recorder);

    let db = Arc::new(Database::open_with_pool_size(
        &config.db_path(&workspace_root),
        config.reader_pool_size,
    )?);

    // Build services
    let graph = Arc::new(GraphEngine::new());
    let indexer = Arc::new(IndexerService::new(
        db.clone(),
        config.clone(),
        workspace_root.clone(),
        extra_roots,
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
        tracing::warn!(error = ?_watcher.err(), "File watcher failed to start");
    }

    // Run initial index asynchronously so MCP clients can connect immediately.
    // This avoids startup timeouts on large repositories.
    let startup_indexer = indexer.clone();
    let startup_graph = graph.clone();
    let startup_db = db.clone();
    let startup_config = config.clone();
    let startup_workspace = workspace_root.clone();
    let startup_skeletonizer = skeletonizer.clone();
    tokio::task::spawn_blocking(move || {
        tracing::info!("Running initial index in background...");
        match startup_indexer.full_index() {
            Ok(report) => {
                tracing::info!(
                    files = report.file_count,
                    nodes = report.node_count,
                    edges = report.edge_count,
                    structure_skipped = report.structure_skip_count,
                    structural_changes = report.structural_changes.len(),
                    "Initial index complete"
                );

                if let Err(e) = startup_db
                    .reader()
                    .and_then(|r| startup_graph.rebuild_from_db(&r))
                {
                    tracing::error!(error = %e, "Graph rebuild after initial index failed");
                } else {
                    tracing::info!(
                        nodes = startup_graph.node_count(),
                        edges = startup_graph.edge_count(),
                        "Graph loaded"
                    );
                }

                // Mark index as ready immediately so MCP tools are available
                startup_indexer.set_index_ready(true);

                // Pre-warm skeleton cache in background (non-blocking)
                if let Err(e) =
                    startup_skeletonizer.prewarm_skeletons(startup_config.default_skeleton_level)
                {
                    tracing::warn!(error = %e, "Skeleton pre-warm failed");
                }

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
                                tracing::info!(edges = resolved, "LSP resolution complete");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "LSP resolution failed");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Initial index failed");
                // Mark ready even on failure so tools don't hang forever
                startup_indexer.set_index_ready(true);
            }
        }
    });

    // Create MCP server
    let server = BexpServer::new(
        db.clone(),
        config.clone(),
        indexer.clone(),
        graph.clone(),
        skeletonizer.clone(),
        capsule.clone(),
        memory.clone(),
        workspace_root,
    );

    // Start health/metrics server if configured
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    if let Some(port) = config.health_port {
        let health_indexer = indexer.clone();
        let health_handle = prometheus_handle.clone();
        tokio::spawn(health::run_health_server(
            port,
            health_indexer,
            health_handle,
            shutdown_rx,
        ));
    } else {
        drop(shutdown_rx);
    }

    tracing::info!("Starting MCP stdio server...");
    let service = server.serve(stdio()).await?;

    // Graceful shutdown: wait for transport close, SIGINT, or SIGTERM
    let shutdown_reason = {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            tokio::select! {
                result = service.waiting() => {
                    if let Err(e) = result {
                        tracing::warn!(error = %e, "MCP transport error");
                    }
                    "transport_close"
                }
                _ = tokio::signal::ctrl_c() => { "SIGINT" }
                _ = sigterm.recv() => { "SIGTERM" }
            }
        }
        #[cfg(not(unix))]
        {
            tokio::select! {
                result = service.waiting() => {
                    if let Err(e) = result {
                        tracing::warn!(error = %e, "MCP transport error");
                    }
                    "transport_close"
                }
                _ = tokio::signal::ctrl_c() => { "SIGINT" }
            }
        }
    };

    tracing::info!(reason = shutdown_reason, "Shutdown initiated");

    // Drain period: let in-flight handlers finish
    if config.shutdown_drain_secs > 0 {
        tracing::info!(
            drain_secs = config.shutdown_drain_secs,
            "Draining in-flight requests"
        );
        tokio::time::sleep(std::time::Duration::from_secs(config.shutdown_drain_secs)).await;
    }

    // Signal health server to stop
    let _ = shutdown_tx.send(true);

    // Flush WAL on all shutdown paths
    tracing::info!("Flushing WAL...");
    if let Err(e) = db.flush_wal() {
        tracing::error!(error = %e, "WAL flush failed");
    }

    Ok(())
}
