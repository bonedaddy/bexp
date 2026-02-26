use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::{BexpServer, QueryNodesParams};
use crate::mcp::validation;

pub async fn handle(
    server: &BexpServer,
    params: QueryNodesParams,
) -> Result<CallToolResult, ErrorData> {
    let limit = validation::validate_limit(params.limit, 50)?;

    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }
    let exported_only = params.exported_only.unwrap_or(false);
    let include_pagerank = params.include_pagerank.unwrap_or(false);

    let reader = server.db.reader();
    let results = queries::query_nodes_filtered(
        &reader,
        params.query.as_deref(),
        params.kind.as_deref(),
        params.file_path.as_deref(),
        params.visibility.as_deref(),
        exported_only,
        limit,
    )
    .map_err(super::to_error_data)?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No symbols found matching the given filters.",
        )]));
    }

    let mut output = format!("# Symbols ({} results)\n\n", results.len());
    for node in &results {
        let export_flag = if node.is_export { " [export]" } else { "" };
        let vis = node.visibility.as_deref().unwrap_or("");
        let vis_str = if vis.is_empty() {
            String::new()
        } else {
            format!(" {vis}")
        };

        output.push_str(&format!(
            "- **`{}`** ({}{}{}) — `{}`:{}-{}\n",
            node.qualified_name.as_deref().unwrap_or(&node.name),
            node.kind,
            vis_str,
            export_flag,
            node.file_path,
            node.line_start,
            node.line_end,
        ));

        if let Some(sig) = &node.signature {
            output.push_str(&format!("  Signature: `{}`\n", sig));
        }
        if let Some(doc) = &node.docstring {
            let short = if doc.len() > 100 {
                format!("{}...", &doc[..97])
            } else {
                doc.clone()
            };
            output.push_str(&format!("  Doc: {}\n", short));
        }
        if include_pagerank {
            if let Some(qn) = &node.qualified_name {
                if let Some(db_id) = server.graph.find_node_index_by_name(qn) {
                    let pr = server.graph.get_pagerank(db_id);
                    output.push_str(&format!("  PageRank: {:.6}\n", pr));
                }
            } else if let Some(db_id) = server.graph.find_node_index_by_name(&node.name) {
                let pr = server.graph.get_pagerank(db_id);
                output.push_str(&format!("  PageRank: {:.6}\n", pr));
            }
        }
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
