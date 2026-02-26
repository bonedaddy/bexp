use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::mpsc;

use crate::config::BexpConfig;
use crate::db::Database;
use crate::graph::GraphEngine;
use crate::indexer::IndexerService;
use crate::types::Language;

pub struct FileWatcher {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl FileWatcher {
    pub fn start(
        workspace_root: PathBuf,
        config: Arc<BexpConfig>,
        indexer: Arc<IndexerService>,
        graph: Arc<GraphEngine>,
        db: Arc<Database>,
    ) -> anyhow::Result<Self> {
        let (tx, mut rx) = mpsc::channel::<Vec<PathBuf>>(100);

        let debounce_ms = config.watcher_debounce_ms;
        let (std_tx, std_rx) = std::sync::mpsc::channel();

        let mut debouncer = new_debouncer(Duration::from_millis(debounce_ms), std_tx)?;

        debouncer
            .watcher()
            .watch(&workspace_root, notify::RecursiveMode::Recursive)?;

        // Also watch extra roots (external workspace_group members)
        let extra_roots: Vec<PathBuf> = indexer.extra_roots().to_vec();
        for extra_root in &extra_roots {
            if let Err(e) = debouncer
                .watcher()
                .watch(extra_root, notify::RecursiveMode::Recursive)
            {
                tracing::warn!(root = %extra_root.display(), error = %e, "Failed to watch extra root");
            }
        }

        // Bridge from std channel to tokio channel
        let config_clone = config.clone();
        let root_clone = workspace_root.clone();
        let extra_roots_clone = extra_roots.clone();
        std::thread::spawn(move || {
            while let Ok(events) = std_rx.recv() {
                match events {
                    Ok(events) => {
                        let unique_paths: HashSet<PathBuf> = events
                            .into_iter()
                            .filter(|e| e.kind == DebouncedEventKind::Any)
                            .map(|e| e.path)
                            .filter(|p| {
                                // Try stripping workspace root or any extra root
                                let rel = p.strip_prefix(&root_clone)
                                    .or_else(|_| {
                                        extra_roots_clone.iter()
                                            .find_map(|r| p.strip_prefix(r).ok())
                                            .ok_or(())
                                    })
                                    .unwrap_or(p);
                                !config_clone.is_excluded(rel)
                            })
                            .filter(|p| {
                                p.extension()
                                    .and_then(|e| e.to_str())
                                    .and_then(Language::from_extension)
                                    .is_some()
                            })
                            .collect();

                        if !unique_paths.is_empty() {
                            let paths: Vec<PathBuf> = unique_paths.into_iter().collect();
                            let _ = tx.blocking_send(paths);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "File watcher error");
                    }
                }
            }
        });

        // Process changes in background
        let indexer_bg = indexer.clone();
        tokio::spawn(async move {
            while let Some(paths) = rx.recv().await {
                tracing::debug!(file_count = paths.len(), "File changes detected");
                match indexer_bg.incremental_reindex(&paths) {
                    Ok(report) => {
                        tracing::info!(
                            files = report.file_count,
                            nodes = report.node_count,
                            edges = report.edge_count,
                            "Incremental reindex complete"
                        );
                        // Incremental graph update instead of full rebuild
                        if !report.changed_file_ids.is_empty() {
                            match db.reader() {
                                Ok(reader) => {
                                    if let Err(e) =
                                        graph.incremental_update(&reader, &report.changed_file_ids)
                                    {
                                        tracing::error!(error = %e, "Incremental graph update failed");
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "Failed to acquire reader lock")
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Incremental reindex failed");
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
