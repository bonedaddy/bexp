use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, Connection};

use crate::config::BexpConfig;
use crate::db::Database;
use crate::error::Result;
use crate::graph::GraphEngine;

use super::client::LspClient;

#[derive(Debug)]
struct UnresolvedRefInfo {
    id: i64,
    source_node_id: i64,
    target_name: String,
    edge_kind: String,
    context: Option<String>,
    line: u32,
    col: u32,
    file_path: String,
}

/// Resolve unresolved references using LSP go-to-definition.
/// Returns the number of edges created.
pub fn resolve_via_lsp(
    db: &Arc<Database>,
    config: &BexpConfig,
    graph: &Arc<GraphEngine>,
    workspace_root: &Path,
) -> Result<usize> {
    if !config.lsp_resolution {
        return Ok(0);
    }

    let conn = db.writer()?;

    // Load unresolved refs with source node locations
    let refs = load_unresolved_refs(&conn)?;

    if refs.is_empty() {
        tracing::info!("No unresolved refs to resolve via LSP");
        return Ok(0);
    }

    tracing::info!(ref_count = refs.len(), "Attempting LSP resolution");

    // Group by file
    let mut by_file: HashMap<String, Vec<&UnresolvedRefInfo>> = HashMap::new();
    for r in &refs {
        by_file.entry(r.file_path.clone()).or_default().push(r);
    }

    let mut total_resolved = 0;

    // Try each configured LSP server
    for (lang_name, server_config) in &config.lsp_servers {
        let workspace_str = workspace_root.to_string_lossy().to_string();

        let mut client =
            match LspClient::spawn(&server_config.command, &server_config.args, &workspace_str) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        lang = %lang_name,
                        command = %server_config.command,
                        error = %e,
                        "Failed to spawn LSP server"
                    );
                    continue;
                }
            };

        if let Err(e) = client.initialize() {
            tracing::warn!(lang = %lang_name, error = %e, "LSP initialize failed");
            continue;
        }

        tracing::info!(lang = %lang_name, "LSP initialized, resolving refs");

        let mut resolved_for_server = 0;

        for (file_path, file_refs) in &by_file {
            let abs_path = workspace_root.join(file_path);
            let abs_str = abs_path.to_string_lossy().to_string();

            for uref in file_refs {
                // Rate limit: don't overwhelm the server
                if resolved_for_server > 0 && resolved_for_server % 50 == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }

                match client.definition(&abs_str, uref.line, uref.col) {
                    Ok(Some(location)) => {
                        // Map location URI back to a relative file path
                        let uri_str = location.uri.as_str();
                        let target_path = uri_str
                            .strip_prefix("file://")
                            .map(PathBuf::from)
                            .and_then(|p| {
                                p.strip_prefix(workspace_root)
                                    .ok()
                                    .map(|r| r.to_string_lossy().to_string())
                            });

                        if let Some(target_rel_path) = target_path {
                            let target_line = location.range.start.line as i64 + 1;

                            if let Some(target_node_id) =
                                find_node_at_location(&conn, &target_rel_path, target_line)
                            {
                                // Insert high-confidence edge
                                let _ = conn.execute(
                                    "INSERT INTO edges (source_node_id, target_node_id, kind, confidence, context)
                                     VALUES (?1, ?2, ?3, 0.99, ?4)",
                                    params![
                                        uref.source_node_id,
                                        target_node_id,
                                        uref.edge_kind,
                                        uref.context,
                                    ],
                                );

                                // Delete resolved ref
                                let _ = conn.execute(
                                    "DELETE FROM unresolved_refs WHERE id = ?1",
                                    params![uref.id],
                                );

                                resolved_for_server += 1;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::debug!(target = %uref.target_name, error = %e, "LSP definition failed");
                    }
                }
            }
        }

        total_resolved += resolved_for_server;
        tracing::info!(
            lang = %lang_name,
            resolved = resolved_for_server,
            total = refs.len(),
            "LSP resolution for server complete"
        );

        let _ = client.shutdown();
    }

    drop(conn);

    // Rebuild graph with new edges
    if total_resolved > 0 {
        let reader = db.reader()?;
        graph.rebuild_from_db(&reader)?;
    }

    tracing::info!(edges = total_resolved, "LSP resolution complete");
    Ok(total_resolved)
}

fn load_unresolved_refs(conn: &Connection) -> Result<Vec<UnresolvedRefInfo>> {
    let mut stmt = conn.prepare(
        "SELECT ur.id, ur.source_node_id, ur.target_name, ur.edge_kind, ur.context,
                n.line_start, n.col_start, f.path
         FROM unresolved_refs ur
         JOIN nodes n ON n.id = ur.source_node_id
         JOIN files f ON f.id = n.file_id
         ORDER BY f.path, n.line_start",
    )?;

    let rows = stmt
        .query_map([], |row| {
            Ok(UnresolvedRefInfo {
                id: row.get(0)?,
                source_node_id: row.get(1)?,
                target_name: row.get(2)?,
                edge_kind: row.get(3)?,
                context: row.get(4)?,
                line: row.get::<_, i64>(5)? as u32 - 1, // Convert to 0-indexed
                col: row.get::<_, i64>(6)? as u32,
                file_path: row.get(7)?,
            })
        })?
        .filter_map(|r| match r {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::trace!(error = %e, "Skipping row due to error");
                None
            }
        })
        .collect();

    Ok(rows)
}

fn find_node_at_location(conn: &Connection, file_path: &str, line: i64) -> Option<i64> {
    conn.query_row(
        "SELECT n.id FROM nodes n
         JOIN files f ON f.id = n.file_id
         WHERE f.path = ?1
           AND n.line_start <= ?2 AND n.line_end >= ?2
         ORDER BY (n.line_end - n.line_start) ASC
         LIMIT 1",
        params![file_path, line],
        |row| row.get(0),
    )
    .ok()
}
