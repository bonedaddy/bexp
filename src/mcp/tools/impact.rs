use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{ImpactParams, VexpServer};

pub async fn handle(
    server: &VexpServer,
    params: ImpactParams,
) -> Result<CallToolResult, ErrorData> {
    let direction = params.direction.as_deref().unwrap_or("both");
    let depth = params.depth.unwrap_or(3);

    let result = server
        .graph
        .impact_graph(&params.symbol, direction, depth)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
