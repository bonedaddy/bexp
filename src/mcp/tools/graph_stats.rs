use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, GraphStatsParams};

pub async fn handle(
    server: &BexpServer,
    params: GraphStatsParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let top_n = params.top_n.unwrap_or(20);

    let node_count = server.graph.node_count();
    let edge_count = server.graph.edge_count();
    let edge_kinds = server.graph.get_edge_kind_counts();
    let top_nodes = server.graph.get_top_pagerank(top_n, params.kind.as_deref());

    let mut output = String::from("# Graph Statistics\n\n");
    output.push_str(&format!("- **Nodes:** {}\n", node_count));
    output.push_str(&format!("- **Edges:** {}\n\n", edge_count));

    if !edge_kinds.is_empty() {
        output.push_str("## Edge Kind Breakdown\n\n");
        for (kind, count) in &edge_kinds {
            output.push_str(&format!("- {}: {}\n", kind, count));
        }
        output.push('\n');
    }

    if !top_nodes.is_empty() {
        let kind_note = if let Some(k) = &params.kind {
            format!(" (kind: {})", k)
        } else {
            String::new()
        };
        output.push_str(&format!(
            "## Top {} by PageRank{}\n\n",
            top_nodes.len(),
            kind_note
        ));
        for (i, (name, kind, score)) in top_nodes.iter().enumerate() {
            output.push_str(&format!(
                "{}. `{}` ({}) — {:.6}\n",
                i + 1,
                name,
                kind,
                score
            ));
        }
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
