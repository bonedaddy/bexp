use rusqlite::{params, Connection};

use crate::error::Result;

/// Resolve unresolved cross-file references by matching target names
/// against known nodes. Returns the number of edges created.
pub fn resolve_cross_file_refs(conn: &Connection) -> Result<usize> {
    let mut count = 0;

    // Get all unresolved refs
    let mut stmt = conn.prepare(
        "SELECT ur.id, ur.source_node_id, ur.target_name, ur.target_qualified_name, ur.edge_kind
         FROM unresolved_refs ur",
    )?;

    let refs: Vec<(i64, i64, String, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut insert_stmt = conn.prepare(
        "INSERT INTO edges (source_node_id, target_node_id, kind, confidence)
         VALUES (?1, ?2, ?3, ?4)",
    )?;

    let mut delete_stmt = conn.prepare("DELETE FROM unresolved_refs WHERE id = ?1")?;

    for (ref_id, source_id, target_name, target_qname, edge_kind) in &refs {
        // First try qualified name match
        let target_id = if let Some(qname) = target_qname {
            find_node_by_qualified_name(conn, qname)
        } else {
            None
        };

        // Fall back to simple name match
        let target_id = target_id.or_else(|| find_node_by_name(conn, target_name, *source_id));

        if let Some(tid) = target_id {
            let confidence = if target_qname.is_some() { 0.95 } else { 0.7 };
            insert_stmt.execute(params![source_id, tid, edge_kind, confidence])?;
            delete_stmt.execute(params![ref_id])?;
            count += 1;
        }
    }

    Ok(count)
}

fn find_node_by_qualified_name(conn: &Connection, qname: &str) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM nodes WHERE qualified_name = ?1 LIMIT 1",
        params![qname],
        |row| row.get(0),
    )
    .ok()
}

fn find_node_by_name(conn: &Connection, name: &str, source_node_id: i64) -> Option<i64> {
    // Find matching nodes that are not in the same file and are exported or public
    conn.query_row(
        "SELECT n.id FROM nodes n
         WHERE n.name = ?1
           AND n.file_id != (SELECT file_id FROM nodes WHERE id = ?2)
           AND (n.is_export = 1 OR n.visibility = 'pub' OR n.visibility = 'public')
         ORDER BY n.id
         LIMIT 1",
        params![name, source_node_id],
        |row| row.get(0),
    )
    .ok()
}
