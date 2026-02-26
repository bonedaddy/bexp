use rusqlite::Connection;

use crate::error::Result;
use crate::types::Intent;

use super::budget::BudgetAllocation;

/// Assemble the final context capsule from the budget allocation.
pub fn assemble_capsule(
    _conn: &Connection,
    allocation: &BudgetAllocation,
    query: &str,
    intent: &Intent,
) -> Result<String> {
    let mut output = String::new();

    // Header
    output.push_str("# Context Capsule\n\n");
    output.push_str(&format!("**Query:** {query}\n"));
    output.push_str(&format!("**Intent:** {}\n", intent.as_str()));
    output.push_str(&format!(
        "**Token usage:** ~{} tokens ({} pivot excerpts, {} bridges, {} skeleton files)\n\n",
        allocation.total_tokens,
        allocation.pivots.len(),
        allocation.bridges.len(),
        allocation.skeletons.len()
    ));

    // Pivot excerpts
    if !allocation.pivots.is_empty() {
        output.push_str("---\n\n## Pivot Files\n\n");
        for pivot in &allocation.pivots {
            let ext = std::path::Path::new(&pivot.path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let location = if pivot.is_full_file {
                format!("`{}`", pivot.path)
            } else {
                format!(
                    "`{}` (lines {}-{})",
                    pivot.path, pivot.line_start, pivot.line_end
                )
            };

            let names = if pivot.node_names.is_empty() {
                String::new()
            } else {
                format!(" — {}", pivot.node_names.join(", "))
            };

            output.push_str(&format!(
                "### {} ({} tokens, relevance: {:.2}){}\n\n```{}\n{}\n```\n\n",
                location, pivot.tokens, pivot.relevance_score, names, ext, pivot.content
            ));
        }
    }

    // Bridge context
    if !allocation.bridges.is_empty() {
        output.push_str("---\n\n## Bridge Context\n\n");
        for bridge in &allocation.bridges {
            output.push_str(&format!(
                "- `{}` in `{}`: `{}`\n",
                bridge.node_name, bridge.path, bridge.signature
            ));
        }
        output.push('\n');
    }

    // Skeleton files
    if !allocation.skeletons.is_empty() {
        output.push_str("---\n\n## Supporting Files (skeletonized)\n\n");
        for skel in &allocation.skeletons {
            let ext = std::path::Path::new(&skel.path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            output.push_str(&format!(
                "### `{}` ({} tokens, {} skeleton)\n\n```{}\n{}\n```\n\n",
                skel.path,
                skel.tokens,
                skel.level.as_str(),
                ext,
                skel.skeleton
            ));
        }
    }

    Ok(output)
}
