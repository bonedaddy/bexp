use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, SkeletonParams};
use crate::types::DetailLevel;

pub async fn handle(
    server: &BexpServer,
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

    let file_path = super::validate_workspace_path(&server.workspace_root, &params.file_path)?;

    let result = server
        .skeletonizer
        .skeletonize(&file_path, level)
        .map_err(super::to_error_data)?;

    Ok(CallToolResult::success(vec![Content::text(result)]))
}
