use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, ObserveParams};

pub async fn handle(
    server: &BexpServer,
    params: ObserveParams,
) -> Result<CallToolResult, ErrorData> {
    let result = server
        .memory
        .save_observation(
            &params.session_id,
            &params.content,
            params.symbols.as_deref(),
            params.files.as_deref(),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
