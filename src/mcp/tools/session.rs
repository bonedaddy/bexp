use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{SessionParams, bexpServer};

pub async fn handle(
    server: &bexpServer,
    params: SessionParams,
) -> Result<CallToolResult, ErrorData> {
    let include_previous = params.include_previous.unwrap_or(false);
    let previous_limit = params.previous_limit.unwrap_or(3);

    let result = server
        .memory
        .get_session_context(
            params.session_id.as_deref(),
            include_previous,
            previous_limit,
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
