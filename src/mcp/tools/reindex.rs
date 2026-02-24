use std::path::PathBuf;

use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{ReindexParams, VexpServer};

pub async fn handle(
    server: &VexpServer,
    params: ReindexParams,
) -> Result<CallToolResult, ErrorData> {
    let report = if params.full.unwrap_or(false) || params.files.is_none() {
        server
            .indexer
            .full_index()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
    } else {
        let paths: Vec<PathBuf> = params
            .files
            .unwrap()
            .iter()
            .map(|f| server.workspace_root.join(f))
            .collect();
        server
            .indexer
            .incremental_reindex(&paths)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
    };

    // Rebuild graph after indexing
    {
        let reader = server.db.reader();
        server
            .graph
            .rebuild_from_db(&reader)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    }

    let output = format!(
        "# Reindex Complete\n\n- **Files:** {}\n- **Nodes:** {}\n- **Edges:** {}",
        report.file_count, report.node_count, report.edge_count,
    );

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
