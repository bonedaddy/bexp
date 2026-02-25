use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::{BexpServer, UnresolvedRefsParams};

pub async fn handle(
    server: &BexpServer,
    params: UnresolvedRefsParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let limit = params.limit.unwrap_or(50);

    let reader = server.db.reader();
    let results =
        queries::get_unresolved_refs_filtered(&reader, params.file_path.as_deref(), limit)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No unresolved references found.",
        )]));
    }

    let mut output = format!("# Unresolved References ({} results)\n\n", results.len());
    for uref in &results {
        let src = uref
            .source_qualified_name
            .as_deref()
            .unwrap_or(&uref.source_name);
        let tgt_qual = uref
            .target_qualified_name
            .as_deref()
            .unwrap_or(&uref.target_name);
        let import = uref
            .import_path
            .as_deref()
            .map(|p| format!(" (import: {})", p))
            .unwrap_or_default();
        output.push_str(&format!(
            "- `{}` → `{}` [{}]{} — in `{}`\n",
            src, tgt_qual, uref.edge_kind, import, uref.source_file,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
