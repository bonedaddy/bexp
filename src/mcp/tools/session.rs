use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{SessionParams, VexpServer};

pub async fn handle(
    server: &VexpServer,
    params: SessionParams,
) -> Result<CallToolResult, ErrorData> {
    let result = server
        .memory
        .get_session_context(params.session_id.as_deref(), params.include_previous.unwrap_or(false))
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
