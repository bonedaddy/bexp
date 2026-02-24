use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{MemorySearchParams, VexpServer};

pub async fn handle(
    server: &VexpServer,
    params: MemorySearchParams,
) -> Result<CallToolResult, ErrorData> {
    let limit = params.limit.unwrap_or(10);

    let result = server
        .memory
        .search(&params.query, limit, params.session_id.as_deref())
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
