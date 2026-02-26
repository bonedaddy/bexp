use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::BexpServer;
use crate::memory::observation;

pub async fn handle(server: &BexpServer) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let conn = server.db.writer();

    let stale_count = observation::detect_staleness(&conn).map_err(super::to_error_data)?;

    let cleanup_count =
        observation::cleanup_old_observations(&conn, server.config.observation_ttl_days)
            .map_err(super::to_error_data)?;

    let output = format!(
        "# Staleness Detection\n\n- **Newly stale:** {} observations marked\n- **Cleaned up:** {} observations past {}-day TTL",
        stale_count, cleanup_count, server.config.observation_ttl_days,
    );

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
