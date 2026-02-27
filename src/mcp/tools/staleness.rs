use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::BexpServer;
use crate::memory::{observation, session};

pub async fn handle(server: &BexpServer) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let conn = server.db.writer().map_err(super::to_error_data)?;

    let stale_count = observation::detect_staleness(&conn).map_err(super::to_error_data)?;

    let cleanup_count =
        observation::cleanup_old_observations(&conn, server.config.observation_ttl_days)
            .map_err(super::to_error_data)?;

    let compressed_count =
        session::compress_stale_sessions(&conn, server.config.session_compress_after_hours)
            .map_err(super::to_error_data)?;

    let output = format!(
        "# Staleness Detection\n\n\
         - **Newly stale:** {} observations marked\n\
         - **Cleaned up:** {} observations past {}-day TTL\n\
         - **Sessions compressed:** {}",
        stale_count,
        cleanup_count,
        server.config.observation_ttl_days,
        compressed_count,
    );

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
