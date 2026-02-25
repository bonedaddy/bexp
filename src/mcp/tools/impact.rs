use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{ImpactParams, bexpServer};

pub async fn handle(
    server: &bexpServer,
    params: ImpactParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let direction = params.direction.as_deref().unwrap_or("both");
    let depth = params.depth.unwrap_or(3);

    let result = server
        .graph
        .impact_graph(
            &params.symbol,
            direction,
            depth,
            params.edge_kinds.as_deref(),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
