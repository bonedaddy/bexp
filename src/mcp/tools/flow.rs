use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{FlowParams, VexpServer};

pub async fn handle(
    server: &VexpServer,
    params: FlowParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let max_depth = params.max_depth.unwrap_or(5);

    let result = server
        .graph
        .find_paths(&params.from_symbol, &params.to_symbol, max_depth)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
