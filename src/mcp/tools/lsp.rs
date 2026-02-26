use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::{BexpServer, LspEdgesParams};
use crate::types::EdgeKind;

pub async fn handle(
    server: &BexpServer,
    params: LspEdgesParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let mut added = 0;
    let mut skipped = 0;

    {
        let conn = server.db.writer();
        let reader = server.db.reader();

        for edge in &params.edges {
            let edge_kind = EdgeKind::parse(&edge.kind).unwrap_or(EdgeKind::Calls);

            let source = find_node_by_qualified_name(&reader, &edge.source_qualified_name);
            let target = find_node_by_qualified_name(&reader, &edge.target_qualified_name);

            match (source, target) {
                (Some(src_id), Some(tgt_id)) => {
                    if queries::insert_edge(&conn, src_id, tgt_id, edge_kind.as_str(), 0.95, None)
                        .is_ok()
                    {
                        added += 1;
                    }
                }
                _ => {
                    skipped += 1;
                }
            }
        }
    }

    // Rebuild graph with new edges
    if added > 0 {
        let reader = server.db.reader();
        server
            .graph
            .rebuild_from_db(&reader)
            .map_err(super::to_error_data)?;
    }

    Ok(CallToolResult::success(vec![Content::text(format!(
        "LSP edges submitted: {} added, {} skipped (unresolved symbols)",
        added, skipped
    ))]))
}

fn find_node_by_qualified_name(conn: &rusqlite::Connection, qualified_name: &str) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM nodes WHERE qualified_name = ?1 LIMIT 1",
        rusqlite::params![qualified_name],
        |row| row.get(0),
    )
    .ok()
}
