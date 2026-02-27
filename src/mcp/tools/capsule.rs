use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, CapsuleParams};
use crate::mcp::validation;

pub async fn handle(
    server: &BexpServer,
    params: CapsuleParams,
) -> Result<CallToolResult, ErrorData> {
    validation::validate_query(&params.query)?;

    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let budget = params.token_budget.unwrap_or(server.config.token_budget);
    let cross_workspace = params.cross_workspace.unwrap_or(true);

    let result = if cross_workspace {
        server
            .capsule
            .generate(
                &params.query,
                budget,
                params.session_id.as_deref(),
                params.intent.as_deref(),
            )
            .map_err(super::to_error_data)?
    } else {
        server
            .capsule
            .generate_with_cross_workspace(
                &params.query,
                budget,
                params.session_id.as_deref(),
                params.intent.as_deref(),
                false,
            )
            .map_err(super::to_error_data)?
    };

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
