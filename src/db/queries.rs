use rusqlite::{params, Connection};

use crate::error::Result;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FileRecord {
    pub id: i64,
    pub path: String,
    pub language: String,
    pub content_hash: String,
    pub mtime_ns: i64,
    pub size_bytes: i64,
    pub token_count: Option<i64>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NodeRecord {
    pub id: i64,
    pub file_id: i64,
    pub kind: String,
    pub name: String,
    pub qualified_name: Option<String>,
    pub signature: Option<String>,
    pub docstring: Option<String>,
    pub line_start: i64,
    pub line_end: i64,
    pub col_start: i64,
    pub col_end: i64,
    pub visibility: Option<String>,
    pub is_export: bool,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EdgeRecord {
    pub id: i64,
    pub source_node_id: i64,
    pub target_node_id: i64,
    pub kind: String,
    pub confidence: f64,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub file_count: i64,
    pub node_count: i64,
    pub edge_count: i64,
    pub unresolved_count: i64,
    pub language_breakdown: Vec<(String, i64)>,
}

pub fn get_index_stats(conn: &Connection) -> Result<IndexStats> {
    let file_count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let node_count: i64 = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
    let edge_count: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
    let unresolved_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM unresolved_refs", [], |r| r.get(0))?;

    let mut stmt = conn
        .prepare("SELECT language, COUNT(*) FROM files GROUP BY language ORDER BY COUNT(*) DESC")?;
    let language_breakdown: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(IndexStats {
        file_count,
        node_count,
        edge_count,
        unresolved_count,
        language_breakdown,
    })
}

#[allow(dead_code)]
pub fn insert_file(
    conn: &Connection,
    path: &str,
    language: &str,
    content_hash: &str,
    mtime_ns: i64,
    size_bytes: i64,
) -> Result<i64> {
    insert_file_with_structure_hash(
        conn,
        path,
        language,
        content_hash,
        mtime_ns,
        size_bytes,
        None,
    )
}

pub fn insert_file_with_structure_hash(
    conn: &Connection,
    path: &str,
    language: &str,
    content_hash: &str,
    mtime_ns: i64,
    size_bytes: i64,
    structure_hash: Option<&str>,
) -> Result<i64> {
    // Delete old nodes first if file exists (cascading delete handles edges)
    conn.execute(
        "DELETE FROM nodes WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
        params![path],
    )?;

    conn.execute(
        "INSERT INTO files (path, language, content_hash, mtime_ns, size_bytes, structure_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET
             language = excluded.language,
             content_hash = excluded.content_hash,
             mtime_ns = excluded.mtime_ns,
             size_bytes = excluded.size_bytes,
             structure_hash = excluded.structure_hash,
             indexed_at = datetime('now'),
             skeleton_minimal = NULL,
             skeleton_standard = NULL,
             skeleton_detailed = NULL",
        params![
            path,
            language,
            content_hash,
            mtime_ns,
            size_bytes,
            structure_hash
        ],
    )?;

    // Get the actual file ID (last_insert_rowid unreliable on upsert)
    let file_id: i64 = conn.query_row(
        "SELECT id FROM files WHERE path = ?1",
        params![path],
        |row| row.get(0),
    )?;
    Ok(file_id)
}

pub fn get_file_by_path(conn: &Connection, path: &str) -> Result<Option<FileRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, language, content_hash, mtime_ns, size_bytes, token_count
         FROM files WHERE path = ?1",
    )?;
    let mut rows = stmt.query_map(params![path], |row| {
        Ok(FileRecord {
            id: row.get(0)?,
            path: row.get(1)?,
            language: row.get(2)?,
            content_hash: row.get(3)?,
            mtime_ns: row.get(4)?,
            size_bytes: row.get(5)?,
            token_count: row.get(6)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn delete_file_data(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute("DELETE FROM nodes WHERE file_id = ?1", params![file_id])?;
    conn.execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_node(
    conn: &Connection,
    file_id: i64,
    kind: &str,
    name: &str,
    qualified_name: Option<&str>,
    signature: Option<&str>,
    docstring: Option<&str>,
    line_start: i64,
    line_end: i64,
    col_start: i64,
    col_end: i64,
    visibility: Option<&str>,
    is_export: bool,
    metadata: Option<&str>,
) -> Result<i64> {
    use crate::db::tokenizer::tokenize_identifier;
    let tokenized_name = tokenize_identifier(name);
    let tokenized_qname = qualified_name.map(tokenize_identifier);

    conn.execute(
        "INSERT INTO nodes (file_id, kind, name, qualified_name, signature, docstring,
                           line_start, line_end, col_start, col_end, visibility, is_export,
                           tokenized_name, tokenized_qualified_name, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            file_id,
            kind,
            name,
            qualified_name,
            signature,
            docstring,
            line_start,
            line_end,
            col_start,
            col_end,
            visibility,
            is_export as i32,
            tokenized_name,
            tokenized_qname,
            metadata,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_edge(
    conn: &Connection,
    source_node_id: i64,
    target_node_id: i64,
    kind: &str,
    confidence: f64,
    context: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO edges (source_node_id, target_node_id, kind, confidence, context)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![source_node_id, target_node_id, kind, confidence, context],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_unresolved_ref(
    conn: &Connection,
    source_node_id: i64,
    target_name: &str,
    target_qualified_name: Option<&str>,
    edge_kind: &str,
    import_path: Option<&str>,
    context: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO unresolved_refs (source_node_id, target_name, target_qualified_name, edge_kind, import_path, context)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![source_node_id, target_name, target_qualified_name, edge_kind, import_path, context],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_all_nodes(conn: &Connection) -> Result<Vec<NodeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, kind, name, qualified_name, signature, docstring,
                line_start, line_end, col_start, col_end, visibility, is_export, metadata
         FROM nodes",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(NodeRecord {
                id: row.get(0)?,
                file_id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                qualified_name: row.get(4)?,
                signature: row.get(5)?,
                docstring: row.get(6)?,
                line_start: row.get(7)?,
                line_end: row.get(8)?,
                col_start: row.get(9)?,
                col_end: row.get(10)?,
                visibility: row.get(11)?,
                is_export: row.get::<_, i32>(12)? != 0,
                metadata: row.get(13)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_all_edges(conn: &Connection) -> Result<Vec<EdgeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_node_id, target_node_id, kind, confidence, context FROM edges",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(EdgeRecord {
                id: row.get(0)?,
                source_node_id: row.get(1)?,
                target_node_id: row.get(2)?,
                kind: row.get(3)?,
                confidence: row.get(4)?,
                context: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[allow(dead_code)]
pub fn search_nodes_fts(conn: &Connection, query: &str, limit: usize) -> Result<Vec<(i64, f64)>> {
    // Tokenize the query so FTS5 can match against tokenized columns
    let tokenized = crate::db::tokenizer::tokenize_query(query);
    let fts_query = if tokenized.is_empty() {
        query.to_string()
    } else {
        tokenized
    };

    search_nodes_fts_raw(conn, &fts_query, limit)
}

/// Execute a pre-sanitized FTS5 query directly (no additional tokenization).
/// Use this when the caller has already built a valid FTS5 query string.
#[allow(dead_code)]
pub fn search_nodes_fts_raw(
    conn: &Connection,
    fts_query: &str,
    limit: usize,
) -> Result<Vec<(i64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT rowid, rank FROM nodes_fts WHERE nodes_fts MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![fts_query, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Combined FTS search + node/file data in a single query.
/// Replaces separate calls to search_nodes_fts_raw and get_nodes_with_files_by_ids.
#[derive(Debug, Clone)]
pub struct FtsSearchResult {
    pub node_id: i64,
    pub rank: f64,
    pub file_id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: String,
    pub signature: Option<String>,
    pub file_path: String,
}

pub fn search_nodes_fts_full(
    conn: &Connection,
    fts_query: &str,
    limit: usize,
) -> Result<Vec<FtsSearchResult>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, nf.rank, n.file_id, n.name, n.qualified_name, n.kind, n.signature, f.path
         FROM nodes_fts nf
         JOIN nodes n ON n.id = nf.rowid
         JOIN files f ON f.id = n.file_id
         WHERE nodes_fts MATCH ?1
         ORDER BY nf.rank
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![fts_query, limit as i64], |row| {
            Ok(FtsSearchResult {
                node_id: row.get(0)?,
                rank: row.get(1)?,
                file_id: row.get(2)?,
                name: row.get(3)?,
                qualified_name: row.get(4)?,
                kind: row.get(5)?,
                signature: row.get(6)?,
                file_path: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[allow(dead_code)]
pub fn get_file_by_id(conn: &Connection, id: i64) -> Result<Option<FileRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, language, content_hash, mtime_ns, size_bytes, token_count
         FROM files WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(FileRecord {
            id: row.get(0)?,
            path: row.get(1)?,
            language: row.get(2)?,
            content_hash: row.get(3)?,
            mtime_ns: row.get(4)?,
            size_bytes: row.get(5)?,
            token_count: row.get(6)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn update_file_skeleton(
    conn: &Connection,
    file_id: i64,
    level: &str,
    skeleton: &str,
    token_count: i64,
) -> Result<()> {
    let (col, tok_col) = match level {
        "minimal" => ("skeleton_minimal", "skeleton_minimal_tokens"),
        "standard" => ("skeleton_standard", "skeleton_standard_tokens"),
        "detailed" => ("skeleton_detailed", "skeleton_detailed_tokens"),
        _ => return Ok(()),
    };
    let sql = format!("UPDATE files SET {col} = ?1, {tok_col} = ?2 WHERE id = ?3");
    conn.execute(&sql, params![skeleton, token_count, file_id])?;
    Ok(())
}

/// Cached skeleton data for batch retrieval.
#[derive(Debug, Clone)]
pub struct SkeletonCacheRow {
    pub path: String,
    pub skeleton_minimal: Option<String>,
    pub skeleton_standard: Option<String>,
    pub skeleton_detailed: Option<String>,
}

/// Batch-fetch file paths and cached skeletons for a set of file IDs.
pub fn get_skeleton_cache_batch(
    conn: &Connection,
    file_ids: &[i64],
) -> Result<std::collections::HashMap<i64, SkeletonCacheRow>> {
    if file_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(",");
    let sql = format!(
        "SELECT id, path, skeleton_minimal, skeleton_standard, skeleton_detailed
         FROM files WHERE id IN ({in_clause})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = file_ids
        .iter()
        .map(|&id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                SkeletonCacheRow {
                    path: row.get(1)?,
                    skeleton_minimal: row.get(2)?,
                    skeleton_standard: row.get(3)?,
                    skeleton_detailed: row.get(4)?,
                },
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Get all files missing a skeleton cache for the given level.
pub fn get_files_missing_skeleton(conn: &Connection, level: &str) -> Result<Vec<FileRecord>> {
    let col = match level {
        "minimal" => "skeleton_minimal",
        "standard" => "skeleton_standard",
        "detailed" => "skeleton_detailed",
        _ => return Ok(Vec::new()),
    };
    let sql = format!(
        "SELECT id, path, language, content_hash, mtime_ns, size_bytes, token_count
         FROM files WHERE {col} IS NULL"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(FileRecord {
            id: row.get(0)?,
            path: row.get(1)?,
            language: row.get(2)?,
            content_hash: row.get(3)?,
            mtime_ns: row.get(4)?,
            size_bytes: row.get(5)?,
            token_count: row.get(6)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn set_metadata(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO index_metadata (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_metadata(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM index_metadata WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get(0))?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

// -- Extended query functions --

pub struct NodeQueryResult {
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub visibility: Option<String>,
    pub is_export: bool,
    pub signature: Option<String>,
    pub docstring: Option<String>,
}

pub fn query_nodes_filtered(
    conn: &Connection,
    query: Option<&str>,
    kind: Option<&str>,
    file_path: Option<&str>,
    visibility: Option<&str>,
    exported_only: bool,
    limit: usize,
) -> Result<Vec<NodeQueryResult>> {
    if let Some(q) = query {
        // FTS5 path: covers name, qualified_name, tokenized variants, signature,
        // docstring, and file_path — a superset of what LIKE searches.
        if let Ok(results) =
            query_nodes_fts_fast(conn, q, kind, file_path, visibility, exported_only, limit)
        {
            // Return FTS results even if empty — no LIKE fallback needed since
            // FTS5 indexes the same columns. Skipping LIKE saves ~10ms on misses.
            return Ok(results);
        }
        // FTS errored (e.g. bad query syntax) — fall through to LIKE
    }

    // No-query path or FTS error: LIKE-based search
    query_nodes_like(
        conn,
        query,
        kind,
        file_path,
        visibility,
        exported_only,
        limit,
    )
}

/// Fast FTS5-based query_nodes. Uses a single FTS5+JOIN query for text matching
/// with kind/visibility/path filters applied directly.
fn query_nodes_fts_fast(
    conn: &Connection,
    query: &str,
    kind: Option<&str>,
    file_path: Option<&str>,
    visibility: Option<&str>,
    exported_only: bool,
    limit: usize,
) -> Result<Vec<NodeQueryResult>> {
    use crate::capsule::search::sanitize_fts_query;

    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    // Restrict FTS to name/qname columns for query_nodes — more selective
    // than searching signature/docstring, giving relevant symbol results.
    let column_restricted = format!(
        "{{name qualified_name tokenized_name tokenized_qualified_name file_path}}: {fts_query}"
    );

    // Single query: FTS5 JOIN nodes JOIN files with all filters applied.
    // This avoids the two-step FTS→IN pattern and its overhead.
    let mut sql = String::from(
        "SELECT n.name, n.qualified_name, n.kind, f.path,
                n.line_start, n.line_end, n.visibility, n.is_export,
                n.signature, n.docstring
         FROM nodes_fts nf
         JOIN nodes n ON n.id = nf.rowid
         JOIN files f ON f.id = n.file_id
         WHERE nodes_fts MATCH ?1",
    );

    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    bind_values.push(Box::new(column_restricted));
    let mut idx = 2;

    if let Some(k) = kind {
        sql.push_str(&format!(" AND n.kind = ?{idx}"));
        bind_values.push(Box::new(k.to_string()));
        idx += 1;
    } else {
        // Exclude imports by default — they're noise
        sql.push_str(" AND n.kind != 'import'");
    }
    if let Some(fp) = file_path {
        sql.push_str(&format!(" AND f.path LIKE ?{idx}"));
        bind_values.push(Box::new(format!("%{fp}%")));
        idx += 1;
    }
    if let Some(vis) = visibility {
        sql.push_str(&format!(" AND n.visibility = ?{idx}"));
        bind_values.push(Box::new(vis.to_string()));
        idx += 1;
    }
    if exported_only {
        sql.push_str(" AND n.is_export = 1");
    }

    sql.push_str(&format!(" ORDER BY nf.rank LIMIT ?{idx}"));
    bind_values.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let results: Vec<NodeQueryResult> = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(NodeQueryResult {
                name: row.get(0)?,
                qualified_name: row.get(1)?,
                kind: row.get(2)?,
                file_path: row.get(3)?,
                line_start: row.get(4)?,
                line_end: row.get(5)?,
                visibility: row.get(6)?,
                is_export: row.get::<_, i32>(7)? != 0,
                signature: row.get(8)?,
                docstring: row.get(9)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// LIKE-based fallback query for query_nodes_filtered (used when no text query
/// is provided or when FTS returns no results).
fn query_nodes_like(
    conn: &Connection,
    query: Option<&str>,
    kind: Option<&str>,
    file_path: Option<&str>,
    visibility: Option<&str>,
    exported_only: bool,
    limit: usize,
) -> Result<Vec<NodeQueryResult>> {
    let mut sql = String::from(
        "SELECT n.name, n.qualified_name, n.kind, f.path,
                n.line_start, n.line_end, n.visibility, n.is_export,
                n.signature, n.docstring",
    );
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;
    let mut where_conditions = Vec::new();

    if let Some(q) = query {
        let terms: Vec<&str> = q.split_whitespace().filter(|t| t.len() > 1).collect();
        if terms.is_empty() {
            where_conditions.push(format!(
                "(n.name LIKE ?{idx} OR n.qualified_name LIKE ?{idx} OR f.path LIKE ?{idx})"
            ));
            bind_values.push(Box::new(format!("%{q}%")));
            idx += 1;
        } else {
            let mut term_match_parts = Vec::new();
            for term in &terms {
                term_match_parts.push(format!(
                    "(n.name LIKE ?{idx} COLLATE NOCASE \
                     OR n.qualified_name LIKE ?{idx} COLLATE NOCASE \
                     OR n.signature LIKE ?{idx} COLLATE NOCASE \
                     OR f.path LIKE ?{idx} COLLATE NOCASE)"
                ));
                bind_values.push(Box::new(format!("%{term}%")));
                idx += 1;
            }
            where_conditions.push(format!("({})", term_match_parts.join(" OR ")));
        }
    }

    sql.push_str(
        " FROM nodes n
         JOIN files f ON f.id = n.file_id
         WHERE 1=1",
    );

    for cond in &where_conditions {
        sql.push_str(&format!(" AND {cond}"));
    }

    if let Some(k) = kind {
        sql.push_str(&format!(" AND n.kind = ?{idx}"));
        bind_values.push(Box::new(k.to_string()));
        idx += 1;
    }
    if let Some(fp) = file_path {
        sql.push_str(&format!(" AND f.path LIKE ?{idx}"));
        bind_values.push(Box::new(format!("%{fp}%")));
        idx += 1;
    }
    if let Some(vis) = visibility {
        sql.push_str(&format!(" AND n.visibility = ?{idx}"));
        bind_values.push(Box::new(vis.to_string()));
        idx += 1;
    }
    if exported_only {
        sql.push_str(" AND n.is_export = 1");
    }
    let _ = idx;

    sql.push_str(&format!(
        " ORDER BY f.path, n.line_start LIMIT ?{}",
        bind_values.len() + 1
    ));
    bind_values.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(NodeQueryResult {
                name: row.get(0)?,
                qualified_name: row.get(1)?,
                kind: row.get(2)?,
                file_path: row.get(3)?,
                line_start: row.get(4)?,
                line_end: row.get(5)?,
                visibility: row.get(6)?,
                is_export: row.get::<_, i32>(7)? != 0,
                signature: row.get(8)?,
                docstring: row.get(9)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub struct EdgeQueryResult {
    pub source_name: String,
    pub source_qualified_name: Option<String>,
    pub source_file: String,
    pub target_name: String,
    pub target_qualified_name: Option<String>,
    pub target_file: String,
    pub kind: String,
    pub confidence: f64,
    pub context: Option<String>,
}

pub fn query_edges_filtered(
    conn: &Connection,
    symbol: Option<&str>,
    kind: Option<&str>,
    min_confidence: Option<f64>,
    direction: Option<&str>,
    limit: usize,
) -> Result<Vec<EdgeQueryResult>> {
    let mut sql = String::from(
        "SELECT sn.name, sn.qualified_name, sf.path,
                tn.name, tn.qualified_name, tf.path,
                e.kind, e.confidence, e.context
         FROM edges e
         JOIN nodes sn ON sn.id = e.source_node_id
         JOIN nodes tn ON tn.id = e.target_node_id
         JOIN files sf ON sf.id = sn.file_id
         JOIN files tf ON tf.id = tn.file_id
         WHERE 1=1",
    );
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(sym) = symbol {
        let dir = direction.unwrap_or("both");
        match dir {
            "outgoing" => {
                sql.push_str(&format!(
                    " AND (sn.name = ?{idx} OR sn.qualified_name = ?{idx})"
                ));
            }
            "incoming" => {
                sql.push_str(&format!(
                    " AND (tn.name = ?{idx} OR tn.qualified_name = ?{idx})"
                ));
            }
            _ => {
                sql.push_str(&format!(
                    " AND (sn.name = ?{idx} OR sn.qualified_name = ?{idx} OR tn.name = ?{idx} OR tn.qualified_name = ?{idx})"
                ));
            }
        }
        bind_values.push(Box::new(sym.to_string()));
        idx += 1;
    }
    if let Some(k) = kind {
        sql.push_str(&format!(" AND e.kind = ?{idx}"));
        bind_values.push(Box::new(k.to_string()));
        idx += 1;
    }
    if let Some(mc) = min_confidence {
        sql.push_str(&format!(" AND e.confidence >= ?{idx}"));
        bind_values.push(Box::new(mc));
        idx += 1;
    }
    sql.push_str(&format!(" ORDER BY e.confidence DESC LIMIT ?{idx}"));
    bind_values.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(EdgeQueryResult {
                source_name: row.get(0)?,
                source_qualified_name: row.get(1)?,
                source_file: row.get(2)?,
                target_name: row.get(3)?,
                target_qualified_name: row.get(4)?,
                target_file: row.get(5)?,
                kind: row.get(6)?,
                confidence: row.get(7)?,
                context: row.get(8)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub struct FileListRecord {
    pub path: String,
    pub language: String,
    pub size_bytes: i64,
    pub token_count: Option<i64>,
    pub content_hash: String,
    pub indexed_at: String,
}

pub fn list_files_filtered(
    conn: &Connection,
    language: Option<&str>,
    sort_by: Option<&str>,
    limit: usize,
) -> Result<Vec<FileListRecord>> {
    let mut sql = String::from(
        "SELECT path, language, size_bytes, token_count, content_hash, indexed_at
         FROM files WHERE 1=1",
    );
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(lang) = language {
        sql.push_str(&format!(" AND language = ?{idx}"));
        bind_values.push(Box::new(lang.to_string()));
        idx += 1;
    }

    let order = match sort_by {
        Some("size") => "size_bytes DESC",
        Some("tokens") => "token_count DESC",
        Some("indexed_at") => "indexed_at DESC",
        _ => "path ASC",
    };
    sql.push_str(&format!(" ORDER BY {order} LIMIT ?{idx}"));
    bind_values.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(FileListRecord {
                path: row.get(0)?,
                language: row.get(1)?,
                size_bytes: row.get(2)?,
                token_count: row.get(3)?,
                content_hash: row.get(4)?,
                indexed_at: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[allow(dead_code)]
pub struct UnresolvedRefRecord {
    pub source_name: String,
    pub source_qualified_name: Option<String>,
    pub source_file: String,
    pub target_name: String,
    pub target_qualified_name: Option<String>,
    pub edge_kind: String,
    pub import_path: Option<String>,
    pub context: Option<String>,
}

pub fn get_unresolved_refs_filtered(
    conn: &Connection,
    file_path: Option<&str>,
    limit: usize,
) -> Result<Vec<UnresolvedRefRecord>> {
    let mut sql = String::from(
        "SELECT sn.name, sn.qualified_name, f.path,
                ur.target_name, ur.target_qualified_name, ur.edge_kind, ur.import_path, ur.context
         FROM unresolved_refs ur
         JOIN nodes sn ON sn.id = ur.source_node_id
         JOIN files f ON f.id = sn.file_id
         WHERE 1=1",
    );
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(fp) = file_path {
        sql.push_str(&format!(" AND f.path LIKE ?{idx}"));
        bind_values.push(Box::new(format!("%{fp}%")));
        idx += 1;
    }
    sql.push_str(&format!(" ORDER BY f.path, sn.name LIMIT ?{idx}"));
    bind_values.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(UnresolvedRefRecord {
                source_name: row.get(0)?,
                source_qualified_name: row.get(1)?,
                source_file: row.get(2)?,
                target_name: row.get(3)?,
                target_qualified_name: row.get(4)?,
                edge_kind: row.get(5)?,
                import_path: row.get(6)?,
                context: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub struct SessionListRecord {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub compressed: bool,
    pub summary: Option<String>,
    pub observation_count: i64,
}

pub fn list_sessions_with_counts(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<SessionListRecord>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.created_at, s.updated_at, s.compressed, s.summary,
                COUNT(o.id) as obs_count
         FROM sessions s
         LEFT JOIN observations o ON o.session_id = s.id
         GROUP BY s.id
         ORDER BY s.updated_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok(SessionListRecord {
                id: row.get(0)?,
                created_at: row.get(1)?,
                updated_at: row.get(2)?,
                compressed: row.get::<_, i32>(3)? != 0,
                summary: row.get(4)?,
                observation_count: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Batch query average edge confidence for a set of node IDs.
/// Returns a map from node_id to average confidence.
pub fn get_avg_edge_confidence_batch(
    conn: &Connection,
    node_ids: &[i64],
) -> Result<std::collections::HashMap<i64, f64>> {
    if node_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<String> = (1..=node_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(",");

    let sql = format!(
        "SELECT node_id, AVG(confidence) FROM (
            SELECT source_node_id AS node_id, confidence FROM edges WHERE source_node_id IN ({in_clause})
            UNION ALL
            SELECT target_node_id AS node_id, confidence FROM edges WHERE target_node_id IN ({in_clause})
        ) GROUP BY node_id"
    );

    let mut stmt = conn.prepare(&sql)?;

    // Numbered placeholders (?1, ?2, ...) are reused across both sub-selects,
    // so we only bind node_ids once.
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for &id in node_ids {
        bind_values.push(Box::new(id));
    }
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Get the current index generation from metadata.
pub fn get_index_generation(conn: &Connection) -> Result<u64> {
    match get_metadata(conn, "index_generation")? {
        Some(v) => Ok(v.parse().unwrap_or(0)),
        None => Ok(0),
    }
}

/// Increment the index generation counter.
pub fn increment_index_generation(conn: &Connection) -> Result<u64> {
    let current = get_index_generation(conn)?;
    let next = current + 1;
    set_metadata(conn, "index_generation", &next.to_string())?;
    Ok(next)
}

// -- Phase 1 query helpers --

/// A candidate node returned by name-based resolution queries.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CandidateNode {
    pub id: i64,
    pub file_id: i64,
    pub kind: String,
    pub name: String,
    pub qualified_name: Option<String>,
    pub signature: Option<String>,
}

/// An import target: a resolved edge from a file to another file's node.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ImportTarget {
    pub target_node_id: i64,
    pub target_file_id: i64,
    pub target_file_path: String,
}

/// A node's range information for budget allocation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NodeRange {
    pub node_id: i64,
    pub file_id: i64,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub kind: String,
    pub name: String,
    pub signature: Option<String>,
}

/// Find ALL exported/pub nodes matching `name` from files other than `source_file_id`.
#[allow(dead_code)]
pub fn find_candidate_nodes_by_name(
    conn: &Connection,
    name: &str,
    source_file_id: i64,
) -> Result<Vec<CandidateNode>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.file_id, n.kind, n.name, n.qualified_name, n.signature
         FROM nodes n
         WHERE n.name = ?1
           AND n.file_id != ?2
           AND (n.is_export = 1 OR n.visibility = 'pub' OR n.visibility = 'public')
         ORDER BY n.id",
    )?;
    let rows = stmt
        .query_map(params![name, source_file_id], |row| {
            Ok(CandidateNode {
                id: row.get(0)?,
                file_id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                qualified_name: row.get(4)?,
                signature: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get all import edges from a file: returns (target_node_id, target_file_id, target_file_path).
#[allow(dead_code)]
pub fn get_file_import_targets(conn: &Connection, file_id: i64) -> Result<Vec<ImportTarget>> {
    let mut stmt = conn.prepare(
        "SELECT e.target_node_id, tn.file_id, f.path
         FROM edges e
         JOIN nodes sn ON sn.id = e.source_node_id
         JOIN nodes tn ON tn.id = e.target_node_id
         JOIN files f ON f.id = tn.file_id
         WHERE sn.file_id = ?1
           AND e.kind = 'imports'",
    )?;
    let rows = stmt
        .query_map(params![file_id], |row| {
            Ok(ImportTarget {
                target_node_id: row.get(0)?,
                target_file_id: row.get(1)?,
                target_file_path: row.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get node ranges by a set of node IDs (for budget allocation).
pub fn get_node_ranges_by_ids(conn: &Connection, node_ids: &[i64]) -> Result<Vec<NodeRange>> {
    if node_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (1..=node_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(",");
    let sql = format!(
        "SELECT n.id, n.file_id, f.path, n.line_start, n.line_end, n.kind, n.name, n.signature
         FROM nodes n
         JOIN files f ON f.id = n.file_id
         WHERE n.id IN ({in_clause})"
    );

    let mut stmt = conn.prepare(&sql)?;
    let bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = node_ids
        .iter()
        .map(|&id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(NodeRange {
                node_id: row.get(0)?,
                file_id: row.get(1)?,
                file_path: row.get(2)?,
                line_start: row.get(3)?,
                line_end: row.get(4)?,
                kind: row.get(5)?,
                name: row.get(6)?,
                signature: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// A node record combined with its file path, for batch hybrid search.
#[derive(Debug, Clone)]
pub struct NodeWithFile {
    pub node_id: i64,
    pub file_id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: String,
    pub signature: Option<String>,
    pub file_path: String,
}

/// Batch-fetch node+file data for a set of node IDs (for hybrid search).
pub fn get_nodes_with_files_by_ids(
    conn: &Connection,
    node_ids: &[i64],
) -> Result<std::collections::HashMap<i64, NodeWithFile>> {
    if node_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<String> = (1..=node_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(",");
    let sql = format!(
        "SELECT n.id, n.file_id, n.name, n.qualified_name, n.kind, n.signature, f.path
         FROM nodes n
         JOIN files f ON f.id = n.file_id
         WHERE n.id IN ({in_clause})"
    );

    let mut stmt = conn.prepare(&sql)?;
    let bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = node_ids
        .iter()
        .map(|&id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(NodeWithFile {
                node_id: row.get(0)?,
                file_id: row.get(1)?,
                name: row.get(2)?,
                qualified_name: row.get(3)?,
                kind: row.get(4)?,
                signature: row.get(5)?,
                file_path: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .map(|nwf| (nwf.node_id, nwf))
        .collect();
    Ok(rows)
}

/// Get total node count for a file.
#[allow(dead_code)]
pub fn get_file_node_count(conn: &Connection, file_id: i64) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM nodes WHERE file_id = ?1",
        params![file_id],
        |r| r.get(0),
    )?;
    Ok(count)
}

/// Batch-fetch node counts for multiple files.
pub fn get_file_node_counts_batch(
    conn: &Connection,
    file_ids: &[i64],
) -> Result<std::collections::HashMap<i64, i64>> {
    if file_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(",");
    let sql = format!(
        "SELECT file_id, COUNT(*) FROM nodes WHERE file_id IN ({in_clause}) GROUP BY file_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = file_ids
        .iter()
        .map(|&id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Get all nodes for the given file IDs (for incremental graph updates).
pub fn get_nodes_for_files(conn: &Connection, file_ids: &[i64]) -> Result<Vec<NodeRecord>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(",");
    let sql = format!(
        "SELECT id, file_id, kind, name, qualified_name, signature, docstring,
                line_start, line_end, col_start, col_end, visibility, is_export, metadata
         FROM nodes
         WHERE file_id IN ({in_clause})"
    );

    let mut stmt = conn.prepare(&sql)?;
    let bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = file_ids
        .iter()
        .map(|&id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(NodeRecord {
                id: row.get(0)?,
                file_id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                qualified_name: row.get(4)?,
                signature: row.get(5)?,
                docstring: row.get(6)?,
                line_start: row.get(7)?,
                line_end: row.get(8)?,
                col_start: row.get(9)?,
                col_end: row.get(10)?,
                visibility: row.get(11)?,
                is_export: row.get::<_, i32>(12)? != 0,
                metadata: row.get(13)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get all file path -> mtime_ns mappings for per-file mtime comparison.
pub fn get_all_file_mtimes(conn: &Connection) -> Result<std::collections::HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT path, mtime_ns FROM files")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Get all file path -> id mappings.
pub fn get_all_file_paths(conn: &Connection) -> Result<std::collections::HashMap<i64, String>> {
    let mut stmt = conn.prepare("SELECT id, path FROM files")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Get all exported/public nodes for batch resolver lookups.
pub fn get_all_exported_nodes(conn: &Connection) -> Result<Vec<CandidateNode>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, kind, name, qualified_name, signature
         FROM nodes
         WHERE is_export = 1 OR visibility = 'pub' OR visibility = 'public'",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CandidateNode {
                id: row.get(0)?,
                file_id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                qualified_name: row.get(4)?,
                signature: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// An import edge with source and target file IDs for batch loading.
#[derive(Debug, Clone)]
pub struct ImportEdgeRecord {
    pub source_file_id: i64,
    pub target_file_id: i64,
}

/// Get all import edges for batch resolver lookups.
pub fn get_all_import_edges(conn: &Connection) -> Result<Vec<ImportEdgeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT sn.file_id, tn.file_id
         FROM edges e
         JOIN nodes sn ON sn.id = e.source_node_id
         JOIN nodes tn ON tn.id = e.target_node_id
         WHERE e.kind = 'imports'",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ImportEdgeRecord {
                source_file_id: row.get(0)?,
                target_file_id: row.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get all edges where either endpoint is in the given node set.
pub fn get_edges_for_nodes(conn: &Connection, node_ids: &[i64]) -> Result<Vec<EdgeRecord>> {
    if node_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (1..=node_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(",");
    let sql = format!(
        "SELECT id, source_node_id, target_node_id, kind, confidence, context
         FROM edges
         WHERE source_node_id IN ({in_clause}) OR target_node_id IN ({in_clause})"
    );

    let mut stmt = conn.prepare(&sql)?;
    // Numbered placeholders (?1, ?2, ...) are reused across both IN clauses,
    // so we only bind node_ids once.
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for &id in node_ids {
        bind_values.push(Box::new(id));
    }
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(EdgeRecord {
                id: row.get(0)?,
                source_node_id: row.get(1)?,
                target_node_id: row.get(2)?,
                kind: row.get(3)?,
                confidence: row.get(4)?,
                context: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get the structure_hash for a file by its ID.
pub fn get_file_structure_hash(conn: &Connection, file_id: i64) -> Result<Option<String>> {
    let result: Option<String> = conn
        .query_row(
            "SELECT structure_hash FROM files WHERE id = ?1",
            params![file_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    Ok(result)
}

use crate::types::NodeSummary;

/// Get a summary of nodes for a file.
pub fn get_nodes_summary_for_file(conn: &Connection, file_id: i64) -> Result<Vec<NodeSummary>> {
    let mut stmt = conn.prepare(
        "SELECT kind, name, signature, line_start, line_end
         FROM nodes WHERE file_id = ?1
         ORDER BY line_start",
    )?;
    let rows = stmt
        .query_map(params![file_id], |row| {
            Ok(NodeSummary {
                kind: row.get(0)?,
                name: row.get(1)?,
                signature: row.get(2)?,
                line_start: row.get(3)?,
                line_end: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Find EnvVar nodes by name.
pub fn find_env_var_nodes(conn: &Connection, var_name: &str) -> Result<Vec<NodeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.file_id, n.kind, n.name, n.qualified_name, n.signature, n.docstring,
                n.line_start, n.line_end, n.col_start, n.col_end, n.visibility, n.is_export, n.metadata
         FROM nodes n
         WHERE n.kind = 'env_var' AND n.name = ?1",
    )?;
    let rows = stmt
        .query_map(params![var_name], |row| {
            Ok(NodeRecord {
                id: row.get(0)?,
                file_id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                qualified_name: row.get(4)?,
                signature: row.get(5)?,
                docstring: row.get(6)?,
                line_start: row.get(7)?,
                line_end: row.get(8)?,
                col_start: row.get(9)?,
                col_end: row.get(10)?,
                visibility: row.get(11)?,
                is_export: row.get::<_, i32>(12)? != 0,
                metadata: row.get(13)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Find all edges of kind 'reads_env' targeting the given env var node IDs,
/// returning the source node + its file path.
pub fn find_env_readers(
    conn: &Connection,
    env_node_ids: &[i64],
) -> Result<Vec<(NodeRecord, String)>> {
    if env_node_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=env_node_ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT n.id, n.file_id, n.kind, n.name, n.qualified_name, n.signature, n.docstring,
                n.line_start, n.line_end, n.col_start, n.col_end, n.visibility, n.is_export, n.metadata,
                f.path
         FROM edges e
         JOIN nodes n ON n.id = e.source_node_id
         JOIN files f ON f.id = n.file_id
         WHERE e.kind = 'reads_env' AND e.target_node_id IN ({})",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for &id in env_node_ids {
        bind_values.push(Box::new(id));
    }
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok((
                NodeRecord {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    kind: row.get(2)?,
                    name: row.get(3)?,
                    qualified_name: row.get(4)?,
                    signature: row.get(5)?,
                    docstring: row.get(6)?,
                    line_start: row.get(7)?,
                    line_end: row.get(8)?,
                    col_start: row.get(9)?,
                    col_end: row.get(10)?,
                    visibility: row.get(11)?,
                    is_export: row.get::<_, i32>(12)? != 0,
                    metadata: row.get(13)?,
                },
                row.get::<_, String>(14)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// List all env var nodes with reader counts.
pub fn list_all_env_vars(conn: &Connection) -> Result<Vec<(String, i64, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT n.name, COUNT(DISTINCT e.source_node_id) as reader_count, n.metadata
         FROM nodes n
         LEFT JOIN edges e ON e.target_node_id = n.id AND e.kind = 'reads_env'
         WHERE n.kind = 'env_var'
         GROUP BY n.name
         ORDER BY reader_count DESC, n.name",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}
