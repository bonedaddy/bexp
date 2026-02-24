use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::mpsc;

use crate::config::VexpConfig;
use crate::db::Database;
use crate::graph::GraphEngine;
use crate::indexer::IndexerService;

pub struct FileWatcher {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl FileWatcher {
    pub fn start(
        workspace_root: PathBuf,
        config: Arc<VexpConfig>,
        indexer: Arc<IndexerService>,
        graph: Arc<GraphEngine>,
        db: Arc<Database>,
    ) -> anyhow::Result<Self> {
        let (tx, mut rx) = mpsc::channel::<Vec<PathBuf>>(100);

        let debounce_ms = config.watcher_debounce_ms;
        let (std_tx, std_rx) = std::sync::mpsc::channel();

        let mut debouncer = new_debouncer(
            Duration::from_millis(debounce_ms),
            std_tx,
        )?;

        debouncer
            .watcher()
            .watch(&workspace_root, notify::RecursiveMode::Recursive)?;

        // Bridge from std channel to tokio channel
        let config_clone = config.clone();
        let root_clone = workspace_root.clone();
        std::thread::spawn(move || {
            while let Ok(events) = std_rx.recv() {
                match events {
                    Ok(events) => {
                        let paths: Vec<PathBuf> = events
                            .into_iter()
                            .filter(|e| e.kind == DebouncedEventKind::Any)
                            .map(|e| e.path)
                            .filter(|p| {
                                let rel = p.strip_prefix(&root_clone).unwrap_or(p);
                                !config_clone.is_excluded(rel)
                            })
                            .collect();

                        if !paths.is_empty() {
                            let _ = tx.blocking_send(paths);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("File watcher error: {:?}", e);
                    }
                }
            }
        });

        // Process changes in background
        let indexer_bg = indexer.clone();
        tokio::spawn(async move {
            while let Some(paths) = rx.recv().await {
                tracing::debug!("File changes detected: {} files", paths.len());
                match indexer_bg.incremental_reindex(&paths) {
                    Ok(report) => {
                        tracing::info!(
                            "Incremental reindex: {} files, {} nodes, {} edges",
                            report.file_count,
                            report.node_count,
                            report.edge_count
                        );
                        // Rebuild graph
                        if let Err(e) = graph.rebuild_from_db(&db.reader()) {
                            tracing::error!("Graph rebuild failed: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Incremental reindex failed: {}", e);
                    }
                }
            }
        });

        indexer.set_watcher_active(true);

        Ok(Self {
            _debouncer: debouncer,
        })
    }
}
