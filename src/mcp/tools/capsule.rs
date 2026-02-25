use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{CapsuleParams, VexpServer};

pub async fn handle(
    server: &VexpServer,
    params: CapsuleParams,
) -> Result<CallToolResult, ErrorData> {
    // Wait for index to be ready (up to 60 seconds)
    if !server.indexer.index_ready() {
        tracing::info!("Waiting for index to be ready before capsule generation...");
        for _ in 0..120 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if server.indexer.index_ready() {
                break;
            }
        }
        if !server.indexer.index_ready() {
            return Ok(CallToolResult::success(vec![Content::text(
                "Index is still building. Try again in a moment, or run `trigger_reindex` with full=true.",
            )]));
        }
    }

    let budget = params.token_budget.unwrap_or(server.config.token_budget);

    let result = server
        .capsule
        .generate(
            &params.query,
            budget,
            params.session_id.as_deref(),
            params.intent.as_deref(),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
