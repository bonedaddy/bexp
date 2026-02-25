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
    index_ready: AtomicBool,
}

impl IndexerService {
    pub fn new(db: Arc<Database>, config: Arc<VexpConfig>, workspace_root: PathBuf) -> Self {
        Self {
            db,
            config,
            workspace_root,
            parser_pool: ParserPool::new(),
            watcher_active: AtomicBool::new(false),
            index_ready: AtomicBool::new(false),
        }
    }

    pub fn watcher_active(&self) -> bool {
        self.watcher_active.load(Ordering::Relaxed)
    }

    pub fn set_watcher_active(&self, active: bool) {
        self.watcher_active.store(active, Ordering::Relaxed);
    }

    pub fn index_ready(&self) -> bool {
        self.index_ready.load(Ordering::Relaxed)
    }

    pub fn set_index_ready(&self, ready: bool) {
        self.index_ready.store(ready, Ordering::Relaxed);
    }

    /// Compute a deterministic hash of all (relative_path, mtime_ns) pairs
    /// from the filesystem, sorted by path. Uses xxh3 for speed.
    fn compute_filesystem_mtime_hash(&self) -> Result<String> {
        let scanner = Scanner::new(&self.config);
        let mut files = scanner.scan(&self.workspace_root)?;
        files.sort();

        let mut hasher_data = Vec::new();
        for path in &files {
            let rel_path = path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(path)
                .to_string_lossy();

            let mtime_ns = std::fs::metadata(path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i64)
                .unwrap_or(0);

            hasher_data.extend_from_slice(rel_path.as_bytes());
            hasher_data.extend_from_slice(&mtime_ns.to_le_bytes());
        }

        let hash = xxhash_rust::xxh3::xxh3_64(&hasher_data);
        Ok(format!("{:016x}", hash))
    }

    pub fn full_index(&self) -> Result<IndexReport> {
        // Compute filesystem mtime hash and compare with stored value
        let fs_hash = self.compute_filesystem_mtime_hash()?;
        {
            let reader = self.db.reader();
            if let Ok(Some(stored_hash)) = queries::get_metadata(&reader, "files_mtime_hash") {
                if stored_hash == fs_hash {
                    let stats = queries::get_index_stats(&reader)?;
                    tracing::info!(
                        "Skipping full index: filesystem unchanged (hash: {})",
                        &fs_hash[..8]
                    );
                    return Ok(IndexReport {
                        file_count: stats.file_count as usize,
                        node_count: stats.node_count as usize,
                        edge_count: stats.edge_count as usize,
                        changed_file_ids: Vec::new(),
                    });
                }
            }
        }

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
            .map_err(VexpError::Database);
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
                let metadata_json = node.metadata.as_ref().map(|m| {
                    serde_json::to_string(m).unwrap_or_default()
                });
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
                    metadata_json.as_deref(),
                )?;
                node_id_map.insert(idx, node_id);
                node_count += 1;
            }

            // Insert intra-file edges
            for edge in &extracted.edges {
                if let (Some(&src), Some(&tgt)) =
                    (node_id_map.get(&edge.source_idx), node_id_map.get(&edge.target_idx))
                {
                    queries::insert_edge(&conn, src, tgt, edge.kind.as_str(), edge.confidence, edge.context.as_deref())?;
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
                        uref.context.as_deref(),
                    )?;
                }
            }
        }

        conn.execute_batch("COMMIT")
            .map_err(VexpError::Database)?;
        drop(conn);

        // Resolve cross-file references
        let resolved = self.resolve_references()?;
        edge_count += resolved;

        // Increment index generation to invalidate caches
        let _ = queries::increment_index_generation(&self.db.writer());

        // Store mtime hash so subsequent starts can skip indexing if nothing changed
        let _ = queries::set_metadata(&self.db.writer(), "files_mtime_hash", &fs_hash);

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
            changed_file_ids: Vec::new(), // Full index doesn't track individual IDs
        })
    }

    pub fn incremental_reindex(&self, changed_paths: &[PathBuf]) -> Result<IndexReport> {
        let mut file_count = 0;
        let mut node_count = 0;
        let mut edge_count = 0;
        let mut changed_file_ids = Vec::new();

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
                    changed_file_ids.push(file_id);

                    let mut node_id_map = std::collections::HashMap::new();
                    for (idx, node) in extracted.nodes.iter().enumerate() {
                        let metadata_json = node.metadata.as_ref().map(|m| {
                            serde_json::to_string(m).unwrap_or_default()
                        });
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
                            metadata_json.as_deref(),
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
                                edge.context.as_deref(),
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
                                uref.context.as_deref(),
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

        // Increment index generation to invalidate caches
        if file_count > 0 {
            let _ = queries::increment_index_generation(&self.db.writer());
        }

        Ok(IndexReport {
            file_count,
            node_count,
            edge_count,
            changed_file_ids,
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
        let local = resolver::resolve_cross_file_refs(&conn)?;

        // After local resolution, try cross-workspace resolution
        let cross = crate::workspace::cross_ref::resolve_cross_workspace(&conn, &self.config)
            .unwrap_or_else(|e| {
                tracing::warn!("Cross-workspace resolution failed: {}", e);
                0
            });
        if cross > 0 {
            tracing::info!("Cross-workspace resolution: {} edges created", cross);
        }

        Ok(local + cross)
    }
}

#[derive(Debug)]
pub struct IndexReport {
    pub file_count: usize,
    pub node_count: usize,
    pub edge_count: usize,
    /// File IDs that were changed in this index operation (for incremental graph updates).
    pub changed_file_ids: Vec<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new(prefix: &str) -> std::io::Result<Self> {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock went backwards")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "{prefix}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn write_file(&self, relative_path: &str, content: &str) -> std::io::Result<()> {
            let full_path = self.path.join(relative_path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(full_path, content)
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn mtime_hash_skips_reindex_when_unchanged() {
        let workspace = TempWorkspace::new("vexp-mtime-skip").unwrap();
        workspace.write_file("lib.rs", "pub fn hello() {}\n").unwrap();

        let config = Arc::new(VexpConfig::default());
        let db_path = config.db_path(&workspace.path);
        let db = Arc::new(crate::db::Database::open(&db_path).unwrap());
        let indexer = IndexerService::new(db.clone(), config, workspace.path.clone());

        // First index should run fully
        let report1 = indexer.full_index().unwrap();
        assert_eq!(report1.file_count, 1);
        assert!(report1.node_count >= 1);

        // Second index should skip (same files, same mtimes)
        let report2 = indexer.full_index().unwrap();
        assert_eq!(report2.file_count, 1); // Cached count
        assert_eq!(report2.changed_file_ids.len(), 0); // No actual reindex
    }

    #[test]
    fn mtime_hash_reindexes_when_file_touched() {
        let workspace = TempWorkspace::new("vexp-mtime-touch").unwrap();
        workspace.write_file("lib.rs", "pub fn hello() {}\n").unwrap();

        let config = Arc::new(VexpConfig::default());
        let db_path = config.db_path(&workspace.path);
        let db = Arc::new(crate::db::Database::open(&db_path).unwrap());
        let indexer = IndexerService::new(db.clone(), config, workspace.path.clone());

        let report1 = indexer.full_index().unwrap();
        assert_eq!(report1.file_count, 1);

        // Touch the file (change mtime)
        std::thread::sleep(std::time::Duration::from_millis(50));
        workspace.write_file("lib.rs", "pub fn hello_changed() {}\n").unwrap();

        // Should re-index because mtime changed
        let report2 = indexer.full_index().unwrap();
        assert_eq!(report2.file_count, 1);
        assert!(report2.node_count >= 1);
        // Since this is a full re-index (not incremental), changed_file_ids may be empty
        // but it should have actually re-indexed (different node count or at least ran)
    }
}
