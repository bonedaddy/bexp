use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{CapsuleParams, VexpServer};

pub async fn handle(
    server: &VexpServer,
    params: CapsuleParams,
) -> Result<CallToolResult, ErrorData> {
    let budget = params.token_budget.unwrap_or(server.config.token_budget);

    let result = server
        .capsule
        .generate(&params.query, budget, params.session_id.as_deref())
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
