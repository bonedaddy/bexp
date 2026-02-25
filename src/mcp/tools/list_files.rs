use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::{BexpServer, ListFilesParams};

pub async fn handle(
    server: &BexpServer,
    params: ListFilesParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let limit = params.limit.unwrap_or(100);

    let reader = server.db.reader();
    let results = queries::list_files_filtered(
        &reader,
        params.language.as_deref(),
        params.sort_by.as_deref(),
        limit,
    )
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No files indexed.",
        )]));
    }

    let mut output = format!("# Indexed Files ({} results)\n\n", results.len());
    for file in &results {
        let tokens = file
            .token_count
            .map(|t| format!("{} tokens", t))
            .unwrap_or_else(|| "n/a".to_string());
        output.push_str(&format!(
            "- `{}` ({}, {} bytes, {}, hash: {}, indexed: {})\n",
            file.path,
            file.language,
            file.size_bytes,
            tokens,
            &file.content_hash[..8.min(file.content_hash.len())],
            file.indexed_at,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
