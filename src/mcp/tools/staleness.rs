use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::memory::observation;
use crate::mcp::server::bexpServer;

pub async fn handle(server: &bexpServer) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let conn = server.db.writer();

    let stale_count = observation::detect_staleness(&conn)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    let cleanup_count =
        observation::cleanup_old_observations(&conn, server.config.observation_ttl_days)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    let output = format!(
        "# Staleness Detection\n\n- **Newly stale:** {} observations marked\n- **Cleaned up:** {} observations past {}-day TTL",
        stale_count, cleanup_count, server.config.observation_ttl_days,
    );

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
