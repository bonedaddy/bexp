use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::mcp::server::{BexpServer, ReindexParams};

pub async fn handle(
    server: &BexpServer,
    params: ReindexParams,
) -> Result<CallToolResult, ErrorData> {
    let report = match params.files {
        Some(ref file_names) if !params.full.unwrap_or(false) => {
            let mut paths = Vec::with_capacity(file_names.len());
            for f in file_names {
                paths.push(super::validate_workspace_path(&server.workspace_root, f)?);
            }
            server
                .indexer
                .incremental_reindex(&paths)
                .map_err(super::to_error_data)?
        }
        _ => server.indexer.full_index().map_err(super::to_error_data)?,
    };

    // Rebuild graph after indexing
    {
        let reader = server.db.reader().map_err(super::to_error_data)?;
        server
            .graph
            .rebuild_from_db(&reader)
            .map_err(super::to_error_data)?;
    }

    let output = format!(
        "# Reindex Complete\n\n- **Files:** {}\n- **Nodes:** {}\n- **Edges:** {}",
        report.file_count, report.node_count, report.edge_count,
    );

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
