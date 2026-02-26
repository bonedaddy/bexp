use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, SessionParams};

pub async fn handle(
    server: &BexpServer,
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
        .map_err(super::to_error_data)?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
