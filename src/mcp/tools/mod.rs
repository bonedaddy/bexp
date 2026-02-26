pub mod capsule;
pub mod flow;
pub mod get_config;
pub mod graph_stats;
pub mod impact;
pub mod list_files;
pub mod list_sessions;
pub mod lsp;
pub mod memory_search;
pub mod observe;
pub mod query_edges;
pub mod query_nodes;
pub mod reindex;
pub mod session;
pub mod setup;
pub mod skeleton;
pub mod staleness;
pub mod status;
pub mod unresolved;

#[cfg(test)]
mod tests;

use std::path::{Component, Path, PathBuf};

use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::indexer::IndexerService;

/// Convert any error into an `ErrorData` with a unique request ID for tracing.
pub fn to_error_data(e: impl std::fmt::Display) -> ErrorData {
    let request_id = uuid::Uuid::new_v4().to_string();
    tracing::error!(request_id = %request_id, error = %e, "MCP tool error");
    ErrorData::internal_error(format!("[{request_id}] {e}"), None)
}

/// Wrap a tool handler with metrics recording.
pub async fn with_metrics<F, Fut>(
    tool_name: &str,
    handler: F,
) -> Result<CallToolResult, ErrorData>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<CallToolResult, ErrorData>>,
{
    crate::metrics::record_tool_call(tool_name);
    let start = std::time::Instant::now();
    let result = handler().await;
    let duration = start.elapsed();
    crate::metrics::record_tool_duration(tool_name, duration);
    if result.is_err() {
        crate::metrics::record_tool_error(tool_name);
    }
    result
}

/// Polls `index_ready()` for up to 60s (120 × 500ms).
/// Returns `Some(result)` with a message if the index is not ready, `None` if ready.
pub async fn wait_for_index(indexer: &IndexerService) -> Option<CallToolResult> {
    if !indexer.index_ready() {
        tracing::info!("Waiting for index to be ready...");
        for _ in 0..120 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if indexer.index_ready() {
                return None;
            }
        }
        return Some(CallToolResult::error(vec![Content::text(
            "Index is still building. Try again in a moment, or run `trigger_reindex` with full=true.",
        )]));
    }
    None
}

/// Validate user-supplied path stays within workspace.
/// Returns canonical absolute path or ErrorData.
pub fn validate_workspace_path(
    workspace_root: &Path,
    user_path: &str,
) -> std::result::Result<PathBuf, rmcp::model::ErrorData> {
    let joined = workspace_root.join(user_path);
    let canonical_root = workspace_root.canonicalize().map_err(|e| {
        rmcp::model::ErrorData::internal_error(
            format!("Cannot canonicalize workspace root: {e}"),
            None,
        )
    })?;
    match joined.canonicalize() {
        Ok(canonical) => {
            if !canonical.starts_with(&canonical_root) {
                return Err(rmcp::model::ErrorData::invalid_params(
                    format!("Path '{}' resolves outside the workspace", user_path),
                    None,
                ));
            }
            Ok(canonical)
        }
        Err(_) => {
            // File doesn't exist — lexical normalization fallback
            let normalized = normalize_path(&joined);
            if !normalized.starts_with(&canonical_root) {
                return Err(rmcp::model::ErrorData::invalid_params(
                    format!("Path '{}' resolves outside the workspace", user_path),
                    None,
                ));
            }
            Ok(normalized)
        }
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if matches!(components.last(), Some(Component::Normal(_))) {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            Component::CurDir => {}
            _ => components.push(component),
        }
    }
    components.iter().collect()
}
