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
    let cross_workspace = params.cross_workspace.unwrap_or(true);

    let reader = server.db.reader().map_err(super::to_error_data)?;
    // The FTS fast path already excludes imports when kind is unset,
    // so we can request exactly the limit we need.
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

    if results.is_empty() && !cross_workspace {
        return Ok(CallToolResult::success(vec![Content::text(
            "No symbols found matching the given filters.",
        )]));
    }

    let local_count = results.len();
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
            let short_sig = if sig.len() > 200 {
                format!("{}…", &sig[..200])
            } else {
                sig.clone()
            };
            output.push_str(&format!("  Signature: `{short_sig}`\n"));
        }
        if let Some(doc) = &node.docstring {
            let short = if doc.len() > 100 {
                format!("{}...", &doc[..97])
            } else {
                doc.clone()
            };
            output.push_str(&format!("  Doc: {short}\n"));
        }
        if include_pagerank {
            if let Some(qn) = &node.qualified_name {
                if let Some(db_id) = server.graph.find_node_index_by_name(qn) {
                    let pr = server.graph.get_pagerank(db_id);
                    output.push_str(&format!("  PageRank: {pr:.6}\n"));
                }
            } else if let Some(db_id) = server.graph.find_node_index_by_name(&node.name) {
                let pr = server.graph.get_pagerank(db_id);
                output.push_str(&format!("  PageRank: {pr:.6}\n"));
            }
        }
    }

    // Cross-workspace: search external DBs for matching symbols
    if cross_workspace && !server.config.workspace_group.is_empty() {
        if let Some(query) = params.query.as_deref() {
            let mut ext_results = Vec::new();
            for ws_path in &server.config.workspace_group {
                let ws_name = std::path::Path::new(ws_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| ws_path.clone());

                let ext_conn = match crate::workspace::open_external_db(ws_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let ext_nodes = queries::query_nodes_filtered(
                    &ext_conn,
                    Some(query),
                    params.kind.as_deref(),
                    params.file_path.as_deref(),
                    params.visibility.as_deref(),
                    exported_only,
                    limit.min(10),
                );

                if let Ok(nodes) = ext_nodes {
                    for node in nodes {
                        ext_results.push((ws_name.clone(), node));
                    }
                }
            }

            if !ext_results.is_empty() {
                output.push_str(&format!(
                    "\n## External Symbols ({} results)\n\n",
                    ext_results.len()
                ));
                for (ws_name, node) in &ext_results {
                    let export_flag = if node.is_export { " [export]" } else { "" };
                    output.push_str(&format!(
                        "- **`{}/{}`** ({}{}){} — `{}`:{}-{}\n",
                        ws_name,
                        node.qualified_name.as_deref().unwrap_or(&node.name),
                        node.kind,
                        export_flag,
                        if let Some(sig) = &node.signature {
                            format!(" `{}`", if sig.len() > 100 { &sig[..100] } else { sig })
                        } else {
                            String::new()
                        },
                        node.file_path,
                        node.line_start,
                        node.line_end,
                    ));
                }
            }
        }
    }

    if local_count == 0 && output.contains("0 results") {
        return Ok(CallToolResult::success(vec![Content::text(
            "No symbols found matching the given filters.",
        )]));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
