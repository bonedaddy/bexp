use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{SkeletonParams, bexpServer};
use crate::types::DetailLevel;

pub async fn handle(
    server: &bexpServer,
    params: SkeletonParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let level = params
        .level
        .as_deref()
        .and_then(DetailLevel::parse)
        .unwrap_or(server.config.default_skeleton_level);

    let file_path = server.workspace_root.join(&params.file_path);

    let result = server
        .skeletonizer
        .skeletonize(&file_path, level)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
