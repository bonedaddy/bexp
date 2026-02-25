use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, MemorySearchParams};

pub async fn handle(
    server: &BexpServer,
    params: MemorySearchParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let limit = params.limit.unwrap_or(10);

    let result = server
        .memory
        .search(&params.query, limit, params.session_id.as_deref())
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
