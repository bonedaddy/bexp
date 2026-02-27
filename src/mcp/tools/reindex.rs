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

    let mut output = format!(
        "# Reindex Complete\n\n- **Files:** {}\n- **Nodes:** {}\n- **Edges:** {}",
        report.file_count, report.node_count, report.edge_count,
    );

    if report.structure_skip_count > 0 {
        output.push_str(&format!(
            "\n- **Structure-unchanged (skipped):** {}",
            report.structure_skip_count,
        ));
    }

    if !report.structural_changes.is_empty() {
        output.push_str("\n\n## Structural Changes\n\n");
        for change in &report.structural_changes {
            output.push_str(&format!("### `{}`\n", change.file_path));
            for added in &change.added_nodes {
                let qn = added
                    .qualified_name
                    .as_deref()
                    .unwrap_or(&added.name);
                output.push_str(&format!(
                    "- **+** {} `{}` (lines {}-{})\n",
                    added.kind, qn, added.line_start, added.line_end
                ));
            }
            for removed in &change.removed_nodes {
                let qn = removed
                    .qualified_name
                    .as_deref()
                    .unwrap_or(&removed.name);
                output.push_str(&format!(
                    "- **-** {} `{}` (lines {}-{})\n",
                    removed.kind, qn, removed.line_start, removed.line_end
                ));
            }
            for modified in &change.modified_nodes {
                let sig_info = match (&modified.old_signature, &modified.new_signature) {
                    (Some(old), Some(new)) => format!(": `{}` -> `{}`", old, new),
                    (None, Some(new)) => format!(": -> `{}`", new),
                    (Some(old), None) => format!(": `{}` -> (none)", old),
                    (None, None) => String::new(),
                };
                output.push_str(&format!(
                    "- **~** {} `{}` signature changed{}\n",
                    modified.kind, modified.name, sig_info
                ));
            }
            for renamed in &change.renamed_nodes {
                output.push_str(&format!(
                    "- **\u{21c4}** {} `{}` \u{2192} `{}` (line {})\n",
                    renamed.kind, renamed.old_name, renamed.new_name, renamed.line_start
                ));
            }
            output.push('\n');
        }
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
