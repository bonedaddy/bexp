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

use rmcp::model::{CallToolResult, Content};

use crate::indexer::IndexerService;

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
