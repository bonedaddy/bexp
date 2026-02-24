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
    output.push_str(&format!("# Context Capsule\n\n"));
    output.push_str(&format!("**Query:** {}\n", query));
    output.push_str(&format!("**Intent:** {}\n", intent.as_str()));
    output.push_str(&format!(
        "**Token usage:** ~{} tokens ({} pivot files, {} skeleton files)\n\n",
        allocation.total_tokens,
        allocation.pivots.len(),
        allocation.skeletons.len()
    ));

    // Pivot files (full content)
    if !allocation.pivots.is_empty() {
        output.push_str("---\n\n## Pivot Files (full content)\n\n");
        for pivot in &allocation.pivots {
            let ext = std::path::Path::new(&pivot.path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            output.push_str(&format!(
                "### `{}` ({} tokens, relevance: {:.2})\n\n```{}\n{}\n```\n\n",
                pivot.path, pivot.tokens, pivot.relevance_score, ext, pivot.content
            ));
        }
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
