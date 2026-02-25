use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::VexpServer;

pub async fn handle(server: &VexpServer) -> Result<CallToolResult, ErrorData> {
    let stats = {
        let reader = server.db.reader();
        queries::get_index_stats(&reader).map_err(|e| ErrorData::internal_error(e.to_string(), None))?
    };

    let watcher_active = server.indexer.watcher_active();

    let mut output = String::new();
    output.push_str("# Vexp Index Status\n\n");
    output.push_str(&format!("- **Files indexed:** {}\n", stats.file_count));
    output.push_str(&format!("- **Symbols (nodes):** {}\n", stats.node_count));
    output.push_str(&format!("- **Edges:** {}\n", stats.edge_count));
    output.push_str(&format!("- **Unresolved refs:** {}\n", stats.unresolved_count));
    output.push_str(&format!(
        "- **File watcher:** {}\n",
        if watcher_active { "active" } else { "inactive" }
    ));

    if !stats.language_breakdown.is_empty() {
        output.push_str("\n## Language Breakdown\n\n");
        for (lang, count) in &stats.language_breakdown {
            output.push_str(&format!("- **{}:** {} files\n", lang, count));
        }
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
