use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, ObserveParams};
use crate::mcp::validation;

pub async fn handle(
    server: &BexpServer,
    params: ObserveParams,
) -> Result<CallToolResult, ErrorData> {
    validation::validate_content(&params.content)?;

    let result = server
        .memory
        .save_observation(
            &params.session_id,
            &params.content,
            params.symbols.as_deref(),
            params.files.as_deref(),
        )
        .map_err(super::to_error_data)?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
