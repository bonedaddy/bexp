use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, ImpactParams};
use crate::mcp::validation;

pub async fn handle(
    server: &BexpServer,
    params: ImpactParams,
) -> Result<CallToolResult, ErrorData> {
    validation::validate_query(&params.symbol)?;
    let direction = params.direction.as_deref().unwrap_or("both");
    validation::validate_direction(direction)?;
    let depth = validation::validate_depth(params.depth, 3)?;

    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let result = server
        .graph
        .impact_graph(
            &params.symbol,
            direction,
            depth,
            params.edge_kinds.as_deref(),
        )
        .map_err(super::to_error_data)?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
