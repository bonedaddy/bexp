use std::collections::HashSet;

use rusqlite::Connection;

use crate::db::queries;
use crate::error::Result;
use crate::skeleton::Skeletonizer;
use crate::types::{DetailLevel, Language};

use super::search::SearchResult;

#[derive(Debug)]
pub struct BudgetAllocation {
    /// Files to include in full
    pub pivots: Vec<PivotFile>,
    /// Files to include as skeletons
    pub skeletons: Vec<SkeletonFile>,
    /// Total tokens used
    pub total_tokens: usize,
}

#[derive(Debug)]
pub struct PivotFile {
    pub file_id: i64,
    pub path: String,
    pub content: String,
    pub tokens: usize,
    pub relevance_score: f64,
}

#[derive(Debug)]
pub struct SkeletonFile {
    pub file_id: i64,
    pub path: String,
    pub skeleton: String,
    pub tokens: usize,
    pub level: DetailLevel,
}

/// Greedy budget allocation: fill pivots first, then skeletons.
pub fn allocate(
    conn: &Connection,
    skeletonizer: &Skeletonizer,
    search_results: &[SearchResult],
    budget: usize,
    default_level: DetailLevel,
) -> Result<BudgetAllocation> {
    let mut allocation = BudgetAllocation {
        pivots: Vec::new(),
        skeletons: Vec::new(),
        total_tokens: 0,
    };

    // Reserve 10% of budget for overhead (headers, formatting)
    let usable_budget = (budget as f64 * 0.9) as usize;
    let mut remaining = usable_budget;

    // Deduplicate files and pick the highest-scoring result per file
    let mut seen_files = HashSet::new();
    let mut file_results: Vec<&SearchResult> = Vec::new();

    for result in search_results {
        if seen_files.insert(result.file_id) {
            file_results.push(result);
        }
    }

    // First pass: try to add top files as full pivots
    let pivot_budget = (usable_budget as f64 * 0.6) as usize;
    let mut pivot_remaining = pivot_budget;

    for result in file_results.iter().take(5) {
        let file = match queries::get_file_by_id(conn, result.file_id)? {
            Some(f) => f,
            None => continue,
        };

        // Read file content
        let content = match std::fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let tokens = skeletonizer.count_tokens(&content);

        if tokens <= pivot_remaining {
            pivot_remaining -= tokens;
            remaining -= tokens;
            allocation.pivots.push(PivotFile {
                file_id: file.id,
                path: file.path.clone(),
                content,
                tokens,
                relevance_score: result.score,
            });
            seen_files.insert(file.id);
        }
    }

    // Second pass: add remaining files as skeletons
    for result in file_results.iter().skip(allocation.pivots.len()) {
        if remaining < 50 {
            break;
        }

        let file = match queries::get_file_by_id(conn, result.file_id)? {
            Some(f) => f,
            None => continue,
        };

        let ext = std::path::Path::new(&file.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let lang = match Language::from_extension(ext) {
            Some(l) => l,
            None => continue,
        };

        // Try standard skeleton first, fall back to minimal
        let (skeleton, level) = {
            let content = match std::fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let std_skel = skeletonizer
                .skeletonize_source(&content, lang, default_level)
                .unwrap_or_default();
            let std_tokens = skeletonizer.count_tokens(&std_skel);

            if std_tokens <= remaining {
                (std_skel, default_level)
            } else {
                let min_skel = skeletonizer
                    .skeletonize_source(&content, lang, DetailLevel::Minimal)
                    .unwrap_or_default();
                let min_tokens = skeletonizer.count_tokens(&min_skel);
                if min_tokens <= remaining {
                    (min_skel, DetailLevel::Minimal)
                } else {
                    continue;
                }
            }
        };

        let tokens = skeletonizer.count_tokens(&skeleton);
        remaining -= tokens;
        allocation.skeletons.push(SkeletonFile {
            file_id: file.id,
            path: file.path.clone(),
            skeleton,
            tokens,
            level,
        });
    }

    allocation.total_tokens = usable_budget - remaining;

    Ok(allocation)
}
