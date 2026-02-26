use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::{BexpServer, ListSessionsParams};
use crate::mcp::validation;

pub async fn handle(
    server: &BexpServer,
    params: ListSessionsParams,
) -> Result<CallToolResult, ErrorData> {
    let limit = validation::validate_limit(params.limit, 20)?;

    let reader = server.db.reader();
    let results =
        queries::list_sessions_with_counts(&reader, limit).map_err(super::to_error_data)?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No sessions found.",
        )]));
    }

    let mut output = format!("# Sessions ({} results)\n\n", results.len());
    for session in &results {
        let compressed = if session.compressed {
            " [compressed]"
        } else {
            ""
        };
        let summary = session
            .summary
            .as_deref()
            .map(|s| format!("\n  Summary: {s}"))
            .unwrap_or_default();
        output.push_str(&format!(
            "- **{}**{} — {} observations\n  Created: {} | Updated: {}{}\n",
            session.id,
            compressed,
            session.observation_count,
            session.created_at,
            session.updated_at,
            summary,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
