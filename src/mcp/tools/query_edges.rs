use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::{BexpServer, QueryEdgesParams};
use crate::mcp::validation;

pub async fn handle(
    server: &BexpServer,
    params: QueryEdgesParams,
) -> Result<CallToolResult, ErrorData> {
    let limit = validation::validate_limit(params.limit, 50)?;

    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let reader = server.db.reader();
    let results = queries::query_edges_filtered(
        &reader,
        params.symbol.as_deref(),
        params.kind.as_deref(),
        params.min_confidence,
        params.direction.as_deref(),
        limit,
    )
    .map_err(super::to_error_data)?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No edges found matching the given filters.",
        )]));
    }

    let mut output = format!("# Edges ({} results)\n\n", results.len());
    for edge in &results {
        let src = edge
            .source_qualified_name
            .as_deref()
            .unwrap_or(&edge.source_name);
        let tgt = edge
            .target_qualified_name
            .as_deref()
            .unwrap_or(&edge.target_name);
        let ctx = edge
            .context
            .as_deref()
            .map(|c| format!(" [{c}]"))
            .unwrap_or_default();
        output.push_str(&format!(
            "- `{}` —[{}]→ `{}` (confidence: {:.2}){}\n  {} → {}\n",
            src, edge.kind, tgt, edge.confidence, ctx, edge.source_file, edge.target_file,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
