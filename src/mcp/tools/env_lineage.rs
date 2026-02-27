use rmcp::model::{CallToolResult, Content, ErrorData};

use crate::db::queries;
use crate::mcp::server::{BexpServer, EnvLineageParams};

pub async fn handle(
    server: &BexpServer,
    params: EnvLineageParams,
) -> Result<CallToolResult, ErrorData> {
    if let Some(result) = super::wait_for_index(&server.indexer).await {
        return Ok(result);
    }

    let reader = server.db.reader().map_err(super::to_error_data)?;

    if params.list_all.unwrap_or(false) {
        // List all env vars
        let all_vars = queries::list_all_env_vars(&reader).map_err(super::to_error_data)?;

        if all_vars.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No environment variables detected in the codebase.",
            )]));
        }

        let mut output = format!("# Environment Variables ({} found)\n\n", all_vars.len());
        for (name, reader_count, metadata) in &all_vars {
            let defined_in = metadata
                .as_ref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get("defined_in").and_then(|d| d.as_str()).map(String::from));

            output.push_str(&format!("- **`{name}`**"));
            if *reader_count > 0 {
                output.push_str(&format!(" — {reader_count} reader(s)"));
            }
            if let Some(def) = &defined_in {
                output.push_str(&format!(" — defined in `{def}`"));
            }
            output.push('\n');
        }

        return Ok(CallToolResult::success(vec![Content::text(output)]));
    }

    // Query specific env var
    let var_name = &params.var_name;
    let env_nodes = queries::find_env_var_nodes(&reader, var_name).map_err(super::to_error_data)?;

    if env_nodes.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(format!(
            "No environment variable `{var_name}` found in the codebase.\n\nTip: Use `list_all: true` to see all detected env vars.",
        ))]));
    }

    let mut output = format!("# Environment Variable: `{var_name}`\n\n");

    // Show definitions (EnvVar nodes from .env files)
    let mut definitions = Vec::new();
    let mut usage_node_ids = Vec::new();

    for node in &env_nodes {
        let file_path: String = reader
            .query_row(
                "SELECT path FROM files WHERE id = ?1",
                rusqlite::params![node.file_id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "unknown".to_string());

        let defined_in = node
            .metadata
            .as_ref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get("defined_in").and_then(|d| d.as_str()).map(String::from));

        if defined_in.is_some() {
            definitions.push((file_path.clone(), defined_in));
        }
        usage_node_ids.push(node.id);
    }

    if !definitions.is_empty() {
        output.push_str("## Definitions\n\n");
        for (path, _defined_in) in &definitions {
            output.push_str(&format!("- `{path}`\n"));
        }
        output.push('\n');
    }

    // Show readers
    let readers =
        queries::find_env_readers(&reader, &usage_node_ids).map_err(super::to_error_data)?;

    if readers.is_empty() {
        output.push_str("## Readers\n\nNo code reads this variable.\n");
    } else {
        output.push_str(&format!("## Readers ({} found)\n\n", readers.len()));
        for (node, file_path) in &readers {
            let sig_info = node
                .signature
                .as_ref()
                .map(|s| {
                    if s.len() > 100 {
                        format!(" — `{}…`", &s[..100])
                    } else {
                        format!(" — `{s}`")
                    }
                })
                .unwrap_or_default();
            output.push_str(&format!(
                "- **`{}`** ({}) in `{}`:{}{}\n",
                node.qualified_name.as_deref().unwrap_or(&node.name),
                node.kind,
                file_path,
                node.line_start,
                sig_info,
            ));
        }
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
