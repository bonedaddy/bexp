use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, FlowParams};
use crate::mcp::validation;

pub async fn handle(server: &BexpServer, params: FlowParams) -> Result<CallToolResult, ErrorData> {
    validation::validate_query(&params.from_symbol)?;
    validation::validate_query(&params.to_symbol)?;
    let max_depth = validation::validate_depth(params.max_depth, 5)?;

    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let result = server
        .graph
        .find_paths(&params.from_symbol, &params.to_symbol, max_depth)
        .map_err(super::to_error_data)?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
