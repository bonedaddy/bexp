use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::VexpServer;

pub async fn handle(server: &VexpServer) -> Result<CallToolResult, ErrorData> {
    let config = &server.config;

    let mut output = String::from("# Vexp Configuration\n\n");
    output.push_str(&format!("- **Token budget:** {}\n", config.token_budget));
    output.push_str(&format!(
        "- **Default skeleton level:** {}\n",
        config.default_skeleton_level.as_str()
    ));
    output.push_str(&format!("- **DB path:** {}\n", config.db_path));
    output.push_str(&format!(
        "- **Max file size:** {} bytes\n",
        config.max_file_size
    ));
    output.push_str(&format!(
        "- **Watcher debounce:** {} ms\n",
        config.watcher_debounce_ms
    ));
    output.push_str(&format!(
        "- **Memory budget:** {:.0}%\n",
        config.memory_budget_pct * 100.0
    ));
    output.push_str(&format!(
        "- **Session compress after:** {} hours\n",
        config.session_compress_after_hours
    ));
    output.push_str(&format!(
        "- **Observation TTL:** {} days\n",
        config.observation_ttl_days
    ));

    if config.exclude_patterns.is_empty() {
        output.push_str("- **Exclude patterns:** (none)\n");
    } else {
        output.push_str(&format!(
            "- **Exclude patterns:** {}\n",
            config.exclude_patterns.join(", ")
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
