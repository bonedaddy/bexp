pub mod extractor;
pub mod languages;
pub mod parser;
pub mod resolver;
pub mod scanner;
pub mod structural_diff;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rayon::prelude::*;

use crate::config::BexpConfig;
use crate::db::queries;
use crate::db::Database;
use crate::error::{BexpError, Result};
use crate::types::Language;

use self::extractor::ExtractedFile;
use self::parser::ParserPool;
use self::scanner::Scanner;

pub struct IndexerService {
    db: Arc<Database>,
    config: Arc<BexpConfig>,
    workspace_root: PathBuf,
    extra_roots: Vec<PathBuf>,
    parser_pool: ParserPool,
    watcher_active: AtomicBool,
    index_ready: AtomicBool,
}

impl IndexerService {
    pub fn new(
        db: Arc<Database>,
        config: Arc<BexpConfig>,
        workspace_root: PathBuf,
        extra_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            db,
            config,
            workspace_root,
            extra_roots,
            parser_pool: ParserPool::new(),
            watcher_active: AtomicBool::new(false),
            index_ready: AtomicBool::new(false),
        }
    }

    /// Compute a relative path for display/storage.
    /// - Files under workspace_root get standard relative paths.
    /// - Files under an extra_root get prefixed with the root's directory name.
    /// - Fallback: absolute path.
    fn relative_path(&self, path: &Path) -> String {
        if let Ok(rel) = path.strip_prefix(&self.workspace_root) {
            return rel.to_string_lossy().to_string();
        }
        for extra_root in &self.extra_roots {
            if let Ok(rel) = path.strip_prefix(extra_root) {
                let prefix = extra_root
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "external".to_string());
                return format!("{}/{}", prefix, rel.to_string_lossy());
            }
        }
        path.to_string_lossy().to_string()
    }

    pub fn extra_roots(&self) -> &[PathBuf] {
        &self.extra_roots
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
        crate::metrics::set_index_ready(ready);
    }

    /// Compute a deterministic hash of all (relative_path, mtime_ns) pairs
    /// from the filesystem, sorted by path. Uses xxh3 for speed.
    fn compute_filesystem_mtime_hash(&self) -> Result<String> {
        let scanner = Scanner::new(&self.config);
        let mut files = scanner.scan(&self.workspace_root)?;
        for extra_root in &self.extra_roots {
            match scanner.scan(extra_root) {
                Ok(extra) => files.extend(extra),
                Err(e) => tracing::warn!(root = %extra_root.display(), error = %e, "Failed to scan extra root for mtime hash"),
            }
        }
        files.sort();

        let mut hasher_data = Vec::new();
        for path in &files {
            let rel_path = self.relative_path(path);

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
        Ok(format!("{hash:016x}"))
    }

    /// Write an extracted file's nodes, edges, and unresolved refs into the database.
    /// Returns (node_count, edge_count) inserted. The caller must have started a
    /// transaction if atomicity across multiple files is needed.
    fn write_extracted_file(
        conn: &rusqlite::Connection,
        file_id: i64,
        extracted: &ExtractedFile,
    ) -> Result<(usize, usize)> {
        let mut node_count = 0;
        let mut edge_count = 0;

        let mut node_id_map = std::collections::HashMap::new();
        for (idx, node) in extracted.nodes.iter().enumerate() {
            let metadata_json = node
                .metadata
                .as_ref()
                .map(|m| serde_json::to_string(m).unwrap_or_default());
            let node_id = queries::insert_node(
                conn,
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
            if let (Some(&src), Some(&tgt)) = (
                node_id_map.get(&edge.source_idx),
                node_id_map.get(&edge.target_idx),
            ) {
                queries::insert_edge(
                    conn,
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
                    conn,
                    src,
                    &uref.target_name,
                    uref.target_qualified_name.as_deref(),
                    uref.edge_kind.as_str(),
                    uref.import_path.as_deref(),
                    uref.context.as_deref(),
                )?;
            }
        }

        Ok((node_count, edge_count))
    }

    pub fn full_index(&self) -> Result<IndexReport> {
        // Compute filesystem mtime hash and compare with stored value
        let fs_hash = self.compute_filesystem_mtime_hash()?;
        {
            let reader = self.db.reader()?;
            if let Ok(Some(stored_hash)) = queries::get_metadata(&reader, "files_mtime_hash") {
                if stored_hash == fs_hash {
                    tracing::info!(
                        hash = &fs_hash[..8],
                        "Skipping full index: filesystem unchanged"
                    );
                    return Ok(IndexReport {
                        file_count: 0,
                        node_count: 0,
                        edge_count: 0,
                        changed_file_ids: Vec::new(),
                        structural_changes: Vec::new(),
                        structure_skip_count: 0,
                    });
                }
            }
        }

        let scanner = Scanner::new(&self.config);
        let mut files = scanner.scan(&self.workspace_root)?;
        for extra_root in &self.extra_roots {
            match scanner.scan(extra_root) {
                Ok(extra_files) => files.extend(extra_files),
                Err(e) => tracing::warn!(root = %extra_root.display(), error = %e, "Failed to scan extra root"),
            }
        }

        tracing::info!(file_count = files.len(), "Scanning found files to index");

        // Load stored mtimes for per-file skip
        let stored_mtimes = {
            let reader = self.db.reader()?;
            queries::get_all_file_mtimes(&reader).unwrap_or_default()
        };

        // Parallel parse — skip files whose mtime hasn't changed
        let extracted: Vec<(PathBuf, ExtractedFile)> = files
            .par_iter()
            .filter_map(|path| {
                // Detect language: try extension first, then check for dotenv
                let lang = if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    Language::from_extension(ext)?
                } else if Scanner::is_dotenv_file(path) {
                    Language::Dotenv
                } else {
                    return None;
                };

                // Per-file mtime check: skip unchanged files
                let rel_path = self.relative_path(path);
                let current_mtime = std::fs::metadata(path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as i64)
                    .unwrap_or(0);
                if let Some(&stored_mtime) = stored_mtimes.get(rel_path.as_str()) {
                    if stored_mtime == current_mtime {
                        return None; // unchanged, skip
                    }
                }

                match self.parse_file(path, lang) {
                    Ok(extracted) => Some((path.clone(), extracted)),
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "Failed to parse file");
                        None
                    }
                }
            })
            .collect();

        tracing::info!(
            changed = extracted.len(),
            total = files.len(),
            "Parsed changed files, writing to database"
        );

        // Sequential batch write — use rusqlite Transaction for automatic rollback on error
        let mut file_count = 0;
        let mut node_count = 0;
        let mut edge_count = 0;
        let mut changed_file_ids = Vec::new();
        let mut structural_changes = Vec::new();
        let mut structure_skip_count = 0;

        {
            let mut conn = self.db.writer()?;
            let tx = conn.transaction().map_err(BexpError::Database)?;

            for (path, extracted) in &extracted {
                let rel_path = self.relative_path(path);

                // Compute structure hash for this file
                let new_structure_hash =
                    extractor::compute_structure_hash(&extracted.nodes, &extracted.edges);

                // Check if structure is unchanged
                if let Ok(Some(old_file)) = queries::get_file_by_path(&tx, &rel_path) {
                    if let Ok(Some(old_hash)) =
                        queries::get_file_structure_hash(&tx, old_file.id)
                    {
                        if old_hash == new_structure_hash {
                            structure_skip_count += 1;
                            // Still update mtime/content_hash but skip node rewrite
                            tx.execute(
                                "UPDATE files SET content_hash = ?1, mtime_ns = ?2, size_bytes = ?3, indexed_at = datetime('now')
                                 WHERE id = ?4",
                                rusqlite::params![
                                    extracted.content_hash,
                                    extracted.mtime_ns,
                                    extracted.size_bytes as i64,
                                    old_file.id
                                ],
                            ).map_err(BexpError::Database)?;
                            continue;
                        } else {
                            // Structure changed — compute diff for report
                            if let Ok(old_nodes) =
                                queries::get_nodes_summary_for_file(&tx, old_file.id)
                            {
                                let new_nodes: Vec<_> = extracted
                                    .nodes
                                    .iter()
                                    .map(|n| {
                                        (
                                            n.kind.as_str().to_string(),
                                            n.name.clone(),
                                            n.signature.clone(),
                                            n.line_start as i64,
                                            n.line_end as i64,
                                        )
                                    })
                                    .collect();
                                let diff = structural_diff::compute_structural_diff(
                                    &rel_path, &old_nodes, &new_nodes,
                                );
                                if !diff.is_empty() {
                                    structural_changes.push(diff);
                                }
                            }
                        }
                    }
                }

                let file_id = queries::insert_file_with_structure_hash(
                    &tx,
                    &rel_path,
                    extracted.language.as_str(),
                    &extracted.content_hash,
                    extracted.mtime_ns,
                    extracted.size_bytes as i64,
                    Some(&new_structure_hash),
                )?;
                file_count += 1;
                changed_file_ids.push(file_id);

                let (nc, ec) = Self::write_extracted_file(&tx, file_id, extracted)?;
                node_count += nc;
                edge_count += ec;
            }

            tx.commit().map_err(BexpError::Database)?;
        }

        // Resolve cross-file references
        let resolved = self.resolve_references()?;
        edge_count += resolved;

        // Increment index generation and store mtime hash
        if let Ok(conn) = self.db.writer() {
            let _ = queries::increment_index_generation(&conn);
            let _ = queries::set_metadata(&conn, "files_mtime_hash", &fs_hash);
        }

        tracing::info!(
            files = file_count,
            nodes = node_count,
            edges = edge_count,
            structure_skips = structure_skip_count,
            "Index complete"
        );
        crate::metrics::record_index_complete(file_count, node_count, edge_count);

        Ok(IndexReport {
            file_count,
            node_count,
            edge_count,
            changed_file_ids,
            structural_changes,
            structure_skip_count,
        })
    }

    pub fn incremental_reindex(&self, changed_paths: &[PathBuf]) -> Result<IndexReport> {
        let mut file_count = 0;
        let mut node_count = 0;
        let mut edge_count = 0;
        let mut changed_file_ids = Vec::new();
        let mut structural_changes = Vec::new();
        let mut structure_skip_count = 0;

        // Phase 1: Parse — check mtimes (reader), parse changed files, collect deletions
        struct Deletion {
            rel_path: String,
        }
        struct ParsedFile {
            rel_path: String,
            extracted: ExtractedFile,
        }

        let mut deletions = Vec::new();
        let mut parsed_files = Vec::new();

        for path in changed_paths {
            let rel_path = self.relative_path(path);

            // Check if file still exists
            if !path.exists() {
                deletions.push(Deletion { rel_path });
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
                let reader = self.db.reader()?;
                if let Ok(Some(file)) = queries::get_file_by_path(&reader, &rel_path) {
                    if file.mtime_ns == mtime_ns {
                        continue; // unchanged
                    }
                }
            }

            // Re-parse
            match self.parse_file(path, lang) {
                Ok(extracted) => {
                    parsed_files.push(ParsedFile {
                        rel_path,
                        extracted,
                    });
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "Failed to re-parse file");
                }
            }
        }

        // Phase 2: Write — single transaction for all deletions and inserts
        // Uses rusqlite Transaction for automatic rollback on error.
        if !deletions.is_empty() || !parsed_files.is_empty() {
            let mut conn = self.db.writer()?;
            let tx = conn.transaction().map_err(BexpError::Database)?;

            for del in &deletions {
                if let Ok(Some(file)) = queries::get_file_by_path(&tx, &del.rel_path) {
                    queries::delete_file_data(&tx, file.id)?;
                }
            }

            for pf in &parsed_files {
                let new_structure_hash =
                    extractor::compute_structure_hash(&pf.extracted.nodes, &pf.extracted.edges);

                // Check if structure is unchanged
                if let Ok(Some(old)) = queries::get_file_by_path(&tx, &pf.rel_path) {
                    if let Ok(Some(old_hash)) = queries::get_file_structure_hash(&tx, old.id) {
                        if old_hash == new_structure_hash {
                            structure_skip_count += 1;
                            tx.execute(
                                "UPDATE files SET content_hash = ?1, mtime_ns = ?2, size_bytes = ?3, indexed_at = datetime('now')
                                 WHERE id = ?4",
                                rusqlite::params![
                                    pf.extracted.content_hash,
                                    pf.extracted.mtime_ns,
                                    pf.extracted.size_bytes as i64,
                                    old.id
                                ],
                            ).map_err(BexpError::Database)?;
                            continue;
                        }
                    }
                    // Structure changed — compute diff
                    if let Ok(old_nodes) = queries::get_nodes_summary_for_file(&tx, old.id) {
                        let new_nodes: Vec<_> = pf
                            .extracted
                            .nodes
                            .iter()
                            .map(|n| {
                                (
                                    n.kind.as_str().to_string(),
                                    n.name.clone(),
                                    n.signature.clone(),
                                    n.line_start as i64,
                                    n.line_end as i64,
                                )
                            })
                            .collect();
                        let diff = structural_diff::compute_structural_diff(
                            &pf.rel_path, &old_nodes, &new_nodes,
                        );
                        if !diff.is_empty() {
                            structural_changes.push(diff);
                        }
                    }

                    queries::delete_file_data(&tx, old.id)?;
                }

                let file_id = queries::insert_file_with_structure_hash(
                    &tx,
                    &pf.rel_path,
                    pf.extracted.language.as_str(),
                    &pf.extracted.content_hash,
                    pf.extracted.mtime_ns,
                    pf.extracted.size_bytes as i64,
                    Some(&new_structure_hash),
                )?;
                file_count += 1;
                changed_file_ids.push(file_id);

                let (nc, ec) = Self::write_extracted_file(&tx, file_id, &pf.extracted)?;
                node_count += nc;
                edge_count += ec;
            }

            tx.commit().map_err(BexpError::Database)?;
        }

        let resolved = self.resolve_references()?;
        edge_count += resolved;

        // Increment index generation to invalidate caches
        if file_count > 0 {
            if let Ok(conn) = self.db.writer() {
                let _ = queries::increment_index_generation(&conn);
            }
        }

        Ok(IndexReport {
            file_count,
            node_count,
            edge_count,
            changed_file_ids,
            structural_changes,
            structure_skip_count,
        })
    }

    fn parse_file(&self, path: &Path, lang: Language) -> Result<ExtractedFile> {
        let content = std::fs::read_to_string(path)?;

        if content.len() > self.config.max_file_size {
            return Err(BexpError::FileTooLarge {
                path: path.display().to_string(),
                size: content.len() as u64,
                max: self.config.max_file_size as u64,
            });
        }

        let metadata = std::fs::metadata(path)?;
        let mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);

        let content_hash = format!("{:016x}", xxhash_rust::xxh3::xxh3_64(content.as_bytes()));
        let rel_path = self.relative_path(path);

        // Dotenv files bypass tree-sitter
        if lang == Language::Dotenv {
            let mut extracted =
                languages::dotenv::extract_dotenv(&content, &rel_path);
            extracted.content_hash = content_hash;
            extracted.mtime_ns = mtime_ns;
            extracted.size_bytes = metadata.len();
            extracted.structure_hash =
                Some(extractor::compute_structure_hash(&extracted.nodes, &extracted.edges));
            return Ok(extracted);
        }

        self.parser_pool.parse(
            &content,
            lang,
            &rel_path,
            content_hash,
            mtime_ns,
            metadata.len(),
        )
    }

    fn resolve_references(&self) -> Result<usize> {
        let conn = self.db.writer()?;
        let local = resolver::resolve_cross_file_refs(&conn)?;

        // After local resolution, try cross-workspace resolution
        let cross = crate::workspace::cross_ref::resolve_cross_workspace(&conn, &self.config)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Cross-workspace resolution failed");
                0
            });
        if cross > 0 {
            tracing::info!(edges = cross, "Cross-workspace resolution created edges");
        }

        // Detect shared types after resolution
        let _shared = resolver::detect_shared_types(&conn).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Shared type detection failed");
            0
        });

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
    /// Structural changes detected during indexing.
    pub structural_changes: Vec<structural_diff::StructuralChange>,
    /// Number of files skipped because structure_hash was unchanged.
    pub structure_skip_count: usize,
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
            let path =
                std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
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
        let workspace = TempWorkspace::new("bexp-mtime-skip").unwrap();
        workspace
            .write_file("lib.rs", "pub fn hello() {}\n")
            .unwrap();

        let config = Arc::new(BexpConfig::default());
        let db_path = config.db_path(&workspace.path);
        let db = Arc::new(crate::db::Database::open(&db_path).unwrap());
        let indexer = IndexerService::new(db.clone(), config, workspace.path.clone(), vec![]);

        // First index should run fully
        let report1 = indexer.full_index().unwrap();
        assert_eq!(report1.file_count, 1);
        assert!(report1.node_count >= 1);

        // Second index should skip (same files, same mtimes)
        let report2 = indexer.full_index().unwrap();
        assert_eq!(report2.file_count, 0); // No files processed
        assert_eq!(report2.changed_file_ids.len(), 0); // No actual reindex
    }

    #[test]
    fn mtime_hash_reindexes_when_file_touched() {
        let workspace = TempWorkspace::new("bexp-mtime-touch").unwrap();
        workspace
            .write_file("lib.rs", "pub fn hello() {}\n")
            .unwrap();

        let config = Arc::new(BexpConfig::default());
        let db_path = config.db_path(&workspace.path);
        let db = Arc::new(crate::db::Database::open(&db_path).unwrap());
        let indexer = IndexerService::new(db.clone(), config, workspace.path.clone(), vec![]);

        let report1 = indexer.full_index().unwrap();
        assert_eq!(report1.file_count, 1);

        // Touch the file (change mtime)
        std::thread::sleep(std::time::Duration::from_millis(50));
        workspace
            .write_file("lib.rs", "pub fn hello_changed() {}\n")
            .unwrap();

        // Should re-index because mtime changed
        let report2 = indexer.full_index().unwrap();
        assert_eq!(report2.file_count, 1);
        assert!(report2.node_count >= 1);
        // Since this is a full re-index (not incremental), changed_file_ids may be empty
        // but it should have actually re-indexed (different node count or at least ran)
    }
}
