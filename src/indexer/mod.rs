pub mod extractor;
pub mod languages;
pub mod parser;
pub mod resolver;
pub mod scanner;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rayon::prelude::*;

use crate::config::VexpConfig;
use crate::db::queries;
use crate::db::Database;
use crate::error::{Result, VexpError};
use crate::types::Language;

use self::extractor::ExtractedFile;
use self::parser::ParserPool;
use self::scanner::Scanner;

pub struct IndexerService {
    db: Arc<Database>,
    config: Arc<VexpConfig>,
    workspace_root: PathBuf,
    parser_pool: ParserPool,
    watcher_active: AtomicBool,
}

impl IndexerService {
    pub fn new(db: Arc<Database>, config: Arc<VexpConfig>, workspace_root: PathBuf) -> Self {
        Self {
            db,
            config,
            workspace_root,
            parser_pool: ParserPool::new(),
            watcher_active: AtomicBool::new(false),
        }
    }

    pub fn watcher_active(&self) -> bool {
        self.watcher_active.load(Ordering::Relaxed)
    }

    pub fn set_watcher_active(&self, active: bool) {
        self.watcher_active.store(active, Ordering::Relaxed);
    }

    pub fn full_index(&self) -> Result<IndexReport> {
        let scanner = Scanner::new(&self.config);
        let files = scanner.scan(&self.workspace_root)?;

        tracing::info!("Scanning found {} files to index", files.len());

        // Parallel parse
        let extracted: Vec<(PathBuf, ExtractedFile)> = files
            .par_iter()
            .filter_map(|path| {
                let ext = path.extension()?.to_str()?;
                let lang = Language::from_extension(ext)?;
                match self.parse_file(path, lang) {
                    Ok(extracted) => Some((path.clone(), extracted)),
                    Err(e) => {
                        tracing::warn!("Failed to parse {}: {}", path.display(), e);
                        None
                    }
                }
            })
            .collect();

        tracing::info!("Parsed {} files, writing to database", extracted.len());

        // Sequential batch write
        let conn = self.db.writer();
        let tx = conn.execute_batch("BEGIN TRANSACTION")
            .map_err(|e| VexpError::Database(e));
        if let Err(e) = tx {
            tracing::error!("Failed to begin transaction: {}", e);
            return Err(e);
        }

        let mut file_count = 0;
        let mut node_count = 0;
        let mut edge_count = 0;

        for (path, extracted) in &extracted {
            let rel_path = path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(path)
                .to_string_lossy();

            let file_id = queries::insert_file(
                &conn,
                &rel_path,
                extracted.language.as_str(),
                &extracted.content_hash,
                extracted.mtime_ns,
                extracted.size_bytes as i64,
            )?;
            file_count += 1;

            // Insert nodes
            let mut node_id_map = std::collections::HashMap::new();
            for (idx, node) in extracted.nodes.iter().enumerate() {
                let node_id = queries::insert_node(
                    &conn,
                    file_id,
                    node.kind.as_str(),
                    &node.name,
                    node.qualified_name.as_deref(),
                    node.signature.as_deref(),
                    node.docstring.as_deref(),
                    node.line_start as i64,
                    node.line_end as i64,
                    node.col_start as i64,
                    node.col_end as i64,
                    node.visibility.as_deref(),
                    node.is_export,
                )?;
                node_id_map.insert(idx, node_id);
                node_count += 1;
            }

            // Insert intra-file edges
            for edge in &extracted.edges {
                if let (Some(&src), Some(&tgt)) =
                    (node_id_map.get(&edge.source_idx), node_id_map.get(&edge.target_idx))
                {
                    queries::insert_edge(&conn, src, tgt, edge.kind.as_str(), edge.confidence)?;
                    edge_count += 1;
                }
            }

            // Insert unresolved references
            for uref in &extracted.unresolved_refs {
                if let Some(&src) = node_id_map.get(&uref.source_idx) {
                    queries::insert_unresolved_ref(
                        &conn,
                        src,
                        &uref.target_name,
                        uref.target_qualified_name.as_deref(),
                        uref.edge_kind.as_str(),
                        uref.import_path.as_deref(),
                    )?;
                }
            }
        }

        conn.execute_batch("COMMIT")
            .map_err(|e| VexpError::Database(e))?;
        drop(conn);

        // Resolve cross-file references
        let resolved = self.resolve_references()?;
        edge_count += resolved;

        tracing::info!(
            "Index complete: {} files, {} nodes, {} edges",
            file_count,
            node_count,
            edge_count
        );

        Ok(IndexReport {
            file_count,
            node_count,
            edge_count,
        })
    }

    pub fn incremental_reindex(&self, changed_paths: &[PathBuf]) -> Result<IndexReport> {
        let mut file_count = 0;
        let mut node_count = 0;
        let mut edge_count = 0;

        for path in changed_paths {
            let rel_path = path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            // Check if file still exists
            if !path.exists() {
                let conn = self.db.writer();
                if let Ok(Some(file)) = queries::get_file_by_path(&conn, &rel_path) {
                    queries::delete_file_data(&conn, file.id)?;
                }
                continue;
            }

            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e,
                None => continue,
            };
            let lang = match Language::from_extension(ext) {
                Some(l) => l,
                None => continue,
            };

            // Check mtime
            let metadata = std::fs::metadata(path)?;
            let mtime_ns = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i64)
                .unwrap_or(0);

            {
                let reader = self.db.reader();
                if let Ok(Some(file)) = queries::get_file_by_path(&reader, &rel_path) {
                    if file.mtime_ns == mtime_ns {
                        continue; // unchanged
                    }
                }
            }

            // Re-parse
            match self.parse_file(path, lang) {
                Ok(extracted) => {
                    let conn = self.db.writer();

                    // Delete old data
                    if let Ok(Some(old)) = queries::get_file_by_path(&conn, &rel_path) {
                        queries::delete_file_data(&conn, old.id)?;
                    }

                    let file_id = queries::insert_file(
                        &conn,
                        &rel_path,
                        extracted.language.as_str(),
                        &extracted.content_hash,
                        extracted.mtime_ns,
                        extracted.size_bytes as i64,
                    )?;
                    file_count += 1;

                    let mut node_id_map = std::collections::HashMap::new();
                    for (idx, node) in extracted.nodes.iter().enumerate() {
                        let node_id = queries::insert_node(
                            &conn,
                            file_id,
                            node.kind.as_str(),
                            &node.name,
                            node.qualified_name.as_deref(),
                            node.signature.as_deref(),
                            node.docstring.as_deref(),
                            node.line_start as i64,
                            node.line_end as i64,
                            node.col_start as i64,
                            node.col_end as i64,
                            node.visibility.as_deref(),
                            node.is_export,
                        )?;
                        node_id_map.insert(idx, node_id);
                        node_count += 1;
                    }

                    for edge in &extracted.edges {
                        if let (Some(&src), Some(&tgt)) =
                            (node_id_map.get(&edge.source_idx), node_id_map.get(&edge.target_idx))
                        {
                            queries::insert_edge(
                                &conn,
                                src,
                                tgt,
                                edge.kind.as_str(),
                                edge.confidence,
                            )?;
                            edge_count += 1;
                        }
                    }

                    for uref in &extracted.unresolved_refs {
                        if let Some(&src) = node_id_map.get(&uref.source_idx) {
                            queries::insert_unresolved_ref(
                                &conn,
                                src,
                                &uref.target_name,
                                uref.target_qualified_name.as_deref(),
                                uref.edge_kind.as_str(),
                                uref.import_path.as_deref(),
                            )?;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to re-parse {}: {}", path.display(), e);
                }
            }
        }

        let resolved = self.resolve_references()?;
        edge_count += resolved;

        Ok(IndexReport {
            file_count,
            node_count,
            edge_count,
        })
    }

    fn parse_file(&self, path: &Path, lang: Language) -> Result<ExtractedFile> {
        let content = std::fs::read_to_string(path)?;

        if content.len() > self.config.max_file_size {
            return Err(VexpError::Index(format!(
                "File too large: {} bytes",
                content.len()
            )));
        }

        let metadata = std::fs::metadata(path)?;
        let mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);

        let content_hash = format!("{:016x}", xxhash_rust::xxh3::xxh3_64(content.as_bytes()));

        let rel_path = path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        self.parser_pool.parse(&content, lang, &rel_path, content_hash, mtime_ns, metadata.len())
    }

    fn resolve_references(&self) -> Result<usize> {
        let conn = self.db.writer();
        resolver::resolve_cross_file_refs(&conn)
    }
}

#[derive(Debug)]
pub struct IndexReport {
    pub file_count: usize,
    pub node_count: usize,
    pub edge_count: usize,
}
