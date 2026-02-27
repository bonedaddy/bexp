use rusqlite::{params, Connection};

use crate::config::BexpConfig;
use crate::error::Result;

use super::open_external_db;

/// Resolve remaining unresolved refs against external workspace databases.
/// Returns the number of cross-workspace edges created.
pub fn resolve_cross_workspace(conn: &Connection, config: &BexpConfig) -> Result<usize> {
    if config.workspace_group.is_empty() {
        return Ok(0);
    }

    // Load remaining unresolved refs
    let mut stmt = conn.prepare(
        "SELECT ur.id, ur.source_node_id, ur.target_name, ur.target_qualified_name, ur.edge_kind
         FROM unresolved_refs ur",
    )?;

    type RefRow = (i64, i64, String, Option<String>, String);
    let refs: Vec<RefRow> = stmt
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

    if refs.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        ref_count = refs.len(),
        workspace_count = config.workspace_group.len(),
        "Attempting cross-workspace resolution"
    );

    let mut insert_stmt = conn.prepare(
        "INSERT INTO cross_workspace_edges (source_node_id, target_workspace, target_qualified_name, kind, confidence)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    let mut delete_stmt = conn.prepare("DELETE FROM unresolved_refs WHERE id = ?1")?;
    let mut total = 0;

    for workspace_path in &config.workspace_group {
        let ext_conn = match open_external_db(workspace_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(workspace = %workspace_path, error = %e, "Cannot open external workspace");
                continue;
            }
        };

        let mut resolved_for_ws = 0;

        for (ref_id, source_id, target_name, target_qname, edge_kind) in &refs {
            // Try qualified name match first
            let found = if let Some(qname) = target_qname {
                find_in_external(&ext_conn, qname, None)
            } else {
                None
            };

            // Fall back to name match
            let found = found.or_else(|| find_in_external(&ext_conn, target_name, Some(true)));

            if let Some(target_qn) = found {
                let confidence = if target_qname.is_some() { 0.85 } else { 0.7 };
                let _ = insert_stmt.execute(params![
                    source_id,
                    workspace_path,
                    target_qn,
                    edge_kind,
                    confidence,
                ]);
                let _ = delete_stmt.execute(params![ref_id]);
                resolved_for_ws += 1;
            }
        }

        if resolved_for_ws > 0 {
            tracing::info!(
                workspace = %workspace_path,
                resolved = resolved_for_ws,
                "Cross-workspace resolution for workspace"
            );
        }
        total += resolved_for_ws;
    }

    tracing::info!(edges = total, "Cross-workspace resolution complete");

    // Mark types involved in cross-workspace edges as shared
    if total > 0 {
        let shared = mark_cross_workspace_shared_types(conn).unwrap_or(0);
        if shared > 0 {
            tracing::info!(count = shared, "Marked cross-workspace shared types");
        }
    }

    Ok(total)
}

/// Find a node in an external workspace's database.
/// If `require_exported` is Some(true), only match exported/public nodes.
/// Returns the qualified_name of the match (for storing in cross_workspace_edges).
fn find_in_external(
    conn: &Connection,
    name: &str,
    require_exported: Option<bool>,
) -> Option<String> {
    // First try qualified_name match
    if let Ok(qn) = conn.query_row(
        "SELECT qualified_name FROM nodes WHERE qualified_name = ?1 LIMIT 1",
        params![name],
        |row| row.get::<_, String>(0),
    ) {
        return Some(qn);
    }

    // Then try name match
    let query = if require_exported == Some(true) {
        "SELECT COALESCE(qualified_name, name) FROM nodes
         WHERE name = ?1
           AND (is_export = 1 OR visibility = 'pub' OR visibility = 'public')
         LIMIT 1"
    } else {
        "SELECT COALESCE(qualified_name, name) FROM nodes WHERE name = ?1 LIMIT 1"
    };

    conn.query_row(query, params![name], |row| row.get(0)).ok()
}

/// Mark source nodes of cross-workspace edges as shared types when they are
/// type-like nodes (interface, type_alias, struct, enum).
fn mark_cross_workspace_shared_types(conn: &Connection) -> Result<usize> {
    let type_kinds = ["interface", "type_alias", "struct", "enum"];
    let mut total = 0;

    for kind in &type_kinds {
        let updated = conn.execute(
            "UPDATE nodes SET metadata = json_set(COALESCE(metadata, '{}'), '$.shared_type', 'true')
             WHERE id IN (
                 SELECT DISTINCT cwe.source_node_id FROM cross_workspace_edges cwe
                 JOIN nodes n ON n.id = cwe.source_node_id
                 WHERE n.kind = ?1
             )",
            params![kind],
        )?;
        total += updated;
    }

    Ok(total)
}
