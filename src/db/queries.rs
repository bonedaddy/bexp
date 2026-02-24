use rusqlite::{params, Connection};

use crate::error::Result;

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
pub struct EdgeRecord {
    pub id: i64,
    pub source_node_id: i64,
    pub target_node_id: i64,
    pub kind: String,
    pub confidence: f64,
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
    let file_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let node_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
    let edge_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
    let unresolved_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM unresolved_refs", [], |r| r.get(0))?;

    let mut stmt =
        conn.prepare("SELECT language, COUNT(*) FROM files GROUP BY language ORDER BY COUNT(*) DESC")?;
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

pub fn insert_file(
    conn: &Connection,
    path: &str,
    language: &str,
    content_hash: &str,
    mtime_ns: i64,
    size_bytes: i64,
) -> Result<i64> {
    // Delete old nodes first if file exists (cascading delete handles edges)
    conn.execute(
        "DELETE FROM nodes WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
        params![path],
    )?;

    conn.execute(
        "INSERT INTO files (path, language, content_hash, mtime_ns, size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(path) DO UPDATE SET
             language = excluded.language,
             content_hash = excluded.content_hash,
             mtime_ns = excluded.mtime_ns,
             size_bytes = excluded.size_bytes,
             indexed_at = datetime('now'),
             skeleton_minimal = NULL,
             skeleton_standard = NULL,
             skeleton_detailed = NULL",
        params![path, language, content_hash, mtime_ns, size_bytes],
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
) -> Result<i64> {
    conn.execute(
        "INSERT INTO nodes (file_id, kind, name, qualified_name, signature, docstring,
                           line_start, line_end, col_start, col_end, visibility, is_export)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            file_id, kind, name, qualified_name, signature, docstring,
            line_start, line_end, col_start, col_end, visibility, is_export as i32,
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
) -> Result<i64> {
    conn.execute(
        "INSERT INTO edges (source_node_id, target_node_id, kind, confidence)
         VALUES (?1, ?2, ?3, ?4)",
        params![source_node_id, target_node_id, kind, confidence],
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
) -> Result<i64> {
    conn.execute(
        "INSERT INTO unresolved_refs (source_node_id, target_name, target_qualified_name, edge_kind, import_path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![source_node_id, target_name, target_qualified_name, edge_kind, import_path],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_all_nodes(conn: &Connection) -> Result<Vec<NodeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, kind, name, qualified_name, signature, docstring,
                line_start, line_end, col_start, col_end, visibility, is_export
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
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_all_edges(conn: &Connection) -> Result<Vec<EdgeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_node_id, target_node_id, kind, confidence FROM edges",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(EdgeRecord {
                id: row.get(0)?,
                source_node_id: row.get(1)?,
                target_node_id: row.get(2)?,
                kind: row.get(3)?,
                confidence: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn search_nodes_fts(conn: &Connection, query: &str, limit: usize) -> Result<Vec<(i64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT rowid, rank FROM nodes_fts WHERE nodes_fts MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_node_by_id(conn: &Connection, id: i64) -> Result<Option<NodeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, kind, name, qualified_name, signature, docstring,
                line_start, line_end, col_start, col_end, visibility, is_export
         FROM nodes WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
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
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

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

pub fn set_metadata(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO index_metadata (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_metadata(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt =
        conn.prepare("SELECT value FROM index_metadata WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get(0))?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}
