use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, MemorySearchParams};
use crate::mcp::validation;

pub async fn handle(
    server: &BexpServer,
    params: MemorySearchParams,
) -> Result<CallToolResult, ErrorData> {
    validation::validate_query(&params.query)?;
    let limit = validation::validate_limit(params.limit, 10)?;

    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let result = server
        .memory
        .search(&params.query, limit, params.session_id.as_deref())
        .map_err(super::to_error_data)?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
