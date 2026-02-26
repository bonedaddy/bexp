use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::db::queries;
use crate::error::Result;
use crate::graph::GraphEngine;
use crate::skeleton::Skeletonizer;
use crate::types::{DetailLevel, Language};

use super::search::SearchResult;

#[derive(Debug)]
pub struct BudgetAllocation {
    /// Excerpt pivots (full file or node-level excerpts)
    pub pivots: Vec<PivotExcerpt>,
    /// Bridge context (signature-only from graph neighbors)
    pub bridges: Vec<BridgeExcerpt>,
    /// Files to include as skeletons
    pub skeletons: Vec<SkeletonFile>,
    /// Total tokens used
    pub total_tokens: usize,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct PivotExcerpt {
    pub file_id: i64,
    pub path: String,
    pub content: String,
    pub tokens: usize,
    pub relevance_score: f64,
    pub line_start: usize,
    pub line_end: usize,
    pub is_full_file: bool,
    pub node_names: Vec<String>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct BridgeExcerpt {
    pub file_id: i64,
    pub path: String,
    pub signature: String,
    pub tokens: usize,
    pub node_name: String,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct SkeletonFile {
    pub file_id: i64,
    pub path: String,
    pub skeleton: String,
    pub tokens: usize,
    pub level: DetailLevel,
}

/// A merged line range within a file.
#[derive(Debug, Clone)]
struct MergedRange {
    line_start: usize,
    line_end: usize,
    node_names: Vec<String>,
}

/// Context lines to pad around each node range.
const CONTEXT_PADDING: usize = 5;

/// Greedy budget allocation with node-level granularity.
///
/// Budget split: 60% pivots, 10% bridges, 30% skeletons.
pub fn allocate(
    conn: &Connection,
    skeletonizer: &Skeletonizer,
    search_results: &[SearchResult],
    budget: usize,
    _default_level: DetailLevel,
    graph: Option<&GraphEngine>,
) -> Result<BudgetAllocation> {
    let mut allocation = BudgetAllocation {
        pivots: Vec::new(),
        bridges: Vec::new(),
        skeletons: Vec::new(),
        total_tokens: 0,
    };

    // Reserve 10% of budget for overhead (headers, formatting)
    let usable_budget = (budget as f64 * 0.9) as usize;
    let pivot_budget = (usable_budget as f64 * 0.6) as usize;
    let bridge_budget = (usable_budget as f64 * 0.1) as usize;
    let skeleton_budget = usable_budget - pivot_budget - bridge_budget;

    let mut pivot_remaining = pivot_budget;
    let mut bridge_remaining = bridge_budget;
    let mut skeleton_remaining = skeleton_budget;

    tracing::debug!(
        total_budget = budget,
        pivot_budget = pivot_budget,
        bridge_budget = bridge_budget,
        skeleton_budget = skeleton_budget,
        "Budget allocation starting"
    );

    // Collect all result node IDs and their scores
    let node_ids: Vec<i64> = search_results.iter().map(|r| r.node_id).collect();

    // Get node ranges for all search result nodes
    let node_ranges = queries::get_node_ranges_by_ids(conn, &node_ids)?;

    // Group by file_id
    let mut file_groups: HashMap<i64, Vec<&queries::NodeRange>> = HashMap::new();
    for nr in &node_ranges {
        file_groups.entry(nr.file_id).or_default().push(nr);
    }

    // Build file score map from search results
    let mut file_scores: HashMap<i64, f64> = HashMap::new();
    for result in search_results {
        file_scores
            .entry(result.file_id)
            .and_modify(|s| {
                if result.score > *s {
                    *s = result.score;
                }
            })
            .or_insert(result.score);
    }

    // Sort files by highest score
    let mut file_order: Vec<i64> = file_groups.keys().copied().collect();
    file_order.sort_by(|a, b| {
        file_scores
            .get(b)
            .unwrap_or(&0.0)
            .partial_cmp(file_scores.get(a).unwrap_or(&0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seen_files = HashSet::new();
    let mut included_node_ids: HashSet<i64> = HashSet::new();

    // Build excerpts for top files
    for &file_id in file_order.iter().take(10) {
        if pivot_remaining < 50 {
            break;
        }
        if !seen_files.insert(file_id) {
            continue;
        }

        let ranges = match file_groups.get(&file_id) {
            Some(r) => r,
            None => continue,
        };

        let file_path = &ranges[0].file_path;
        let score = file_scores.get(&file_id).copied().unwrap_or(0.0);

        // Read file content
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let total_lines = content.lines().count();

        // Get total node count for this file
        let total_node_count = queries::get_file_node_count(conn, file_id).unwrap_or(0) as usize;
        let matched_count = ranges.len();

        // Decide: full file or excerpt
        let use_full_file =
            total_lines < 50 || (total_node_count > 0 && matched_count * 2 >= total_node_count);

        if use_full_file {
            let tokens = skeletonizer.count_tokens(&content);
            if tokens <= pivot_remaining {
                for r in ranges {
                    included_node_ids.insert(r.node_id);
                }
                pivot_remaining -= tokens;
                allocation.pivots.push(PivotExcerpt {
                    file_id,
                    path: file_path.clone(),
                    content,
                    tokens,
                    relevance_score: score,
                    line_start: 1,
                    line_end: total_lines,
                    is_full_file: true,
                    node_names: ranges.iter().map(|r| r.name.clone()).collect(),
                });
            }
        } else {
            // Merge node ranges with padding
            let raw_ranges: Vec<(usize, usize, String)> = ranges
                .iter()
                .map(|r| (r.line_start as usize, r.line_end as usize, r.name.clone()))
                .collect();
            let merged = merge_ranges(&raw_ranges, total_lines);

            for mr in merged {
                let excerpt = extract_lines(&content, mr.line_start, mr.line_end);
                let tokens = skeletonizer.count_tokens(&excerpt);
                if tokens <= pivot_remaining {
                    for r in ranges {
                        if (r.line_start as usize) >= mr.line_start
                            && (r.line_end as usize) <= mr.line_end
                        {
                            included_node_ids.insert(r.node_id);
                        }
                    }
                    pivot_remaining -= tokens;
                    allocation.pivots.push(PivotExcerpt {
                        file_id,
                        path: file_path.clone(),
                        content: excerpt,
                        tokens,
                        relevance_score: score,
                        line_start: mr.line_start,
                        line_end: mr.line_end,
                        is_full_file: false,
                        node_names: mr.node_names,
                    });
                }
            }
        }
    }

    tracing::debug!(
        pivot_count = allocation.pivots.len(),
        tokens_used = pivot_budget - pivot_remaining,
        "Pivot allocation complete"
    );

    // Phase 3B: Bridge excerpts from graph neighbors
    if let Some(graph) = graph {
        let pivot_node_ids: HashSet<i64> = included_node_ids.clone();
        let bridge_candidates = graph.get_bridge_candidates(&pivot_node_ids, &included_node_ids);

        // Get node info for bridge candidates
        let bridge_ranges = queries::get_node_ranges_by_ids(conn, &bridge_candidates)?;

        for nr in &bridge_ranges {
            if bridge_remaining < 20 {
                break;
            }
            let sig = nr.signature.as_deref().unwrap_or(&nr.name);
            let tokens = skeletonizer.count_tokens(sig);
            if tokens <= bridge_remaining {
                bridge_remaining -= tokens;
                allocation.bridges.push(BridgeExcerpt {
                    file_id: nr.file_id,
                    path: nr.file_path.clone(),
                    signature: sig.to_string(),
                    tokens,
                    node_name: nr.name.clone(),
                });
                included_node_ids.insert(nr.node_id);
            }
        }
    }

    tracing::debug!(
        bridge_count = allocation.bridges.len(),
        tokens_used = bridge_budget - bridge_remaining,
        "Bridge allocation complete"
    );

    // Skeleton files: remaining files from search results not already included
    let max_score = search_results.first().map(|r| r.score).unwrap_or(1.0);
    let high_threshold = max_score * 0.7;
    let medium_threshold = max_score * 0.4;

    // Deduplicate and get remaining files
    let mut skeleton_files: Vec<(i64, f64)> = Vec::new();
    let mut skel_seen = HashSet::new();
    for result in search_results {
        if !seen_files.contains(&result.file_id) && skel_seen.insert(result.file_id) {
            skeleton_files.push((result.file_id, result.score));
        }
    }

    for (file_id, score) in skeleton_files {
        if skeleton_remaining < 50 {
            break;
        }

        let file = match queries::get_file_by_id(conn, file_id)? {
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

        let preferred_level = if score >= high_threshold {
            DetailLevel::Detailed
        } else if score >= medium_threshold {
            DetailLevel::Standard
        } else {
            DetailLevel::Minimal
        };

        let (skeleton, level) = {
            let content = match std::fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let levels_to_try = match preferred_level {
                DetailLevel::Detailed => vec![
                    DetailLevel::Detailed,
                    DetailLevel::Standard,
                    DetailLevel::Minimal,
                ],
                DetailLevel::Standard => vec![DetailLevel::Standard, DetailLevel::Minimal],
                DetailLevel::Minimal => vec![DetailLevel::Minimal],
            };

            let mut chosen = None;
            for level in levels_to_try {
                let skel = skeletonizer
                    .skeletonize_source(&content, lang, level)
                    .unwrap_or_default();
                let tok = skeletonizer.count_tokens(&skel);
                if tok <= skeleton_remaining {
                    chosen = Some((skel, level));
                    break;
                }
            }

            match chosen {
                Some(c) => c,
                None => continue,
            }
        };

        let tokens = skeletonizer.count_tokens(&skeleton);
        skeleton_remaining -= tokens;
        allocation.skeletons.push(SkeletonFile {
            file_id,
            path: file.path.clone(),
            skeleton,
            tokens,
            level,
        });
    }

    tracing::debug!(
        skeleton_count = allocation.skeletons.len(),
        tokens_used = skeleton_budget - skeleton_remaining,
        "Skeleton allocation complete"
    );

    allocation.total_tokens = (pivot_budget - pivot_remaining)
        + (bridge_budget - bridge_remaining)
        + (skeleton_budget - skeleton_remaining);

    Ok(allocation)
}

/// Merge overlapping/adjacent ranges after adding context padding.
fn merge_ranges(ranges: &[(usize, usize, String)], total_lines: usize) -> Vec<MergedRange> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut padded: Vec<(usize, usize, String)> = ranges
        .iter()
        .map(|(start, end, name)| {
            let padded_start = start.saturating_sub(CONTEXT_PADDING).max(1);
            let padded_end = (*end + CONTEXT_PADDING).min(total_lines);
            (padded_start, padded_end, name.clone())
        })
        .collect();

    padded.sort_by_key(|r| r.0);

    let mut merged: Vec<MergedRange> = Vec::new();
    let (first_start, first_end, first_name) = padded[0].clone();
    merged.push(MergedRange {
        line_start: first_start,
        line_end: first_end,
        node_names: vec![first_name],
    });

    for (start, end, name) in padded.into_iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if start <= last.line_end + 1 {
            // Overlapping or adjacent — merge
            last.line_end = last.line_end.max(end);
            last.node_names.push(name);
        } else {
            merged.push(MergedRange {
                line_start: start,
                line_end: end,
                node_names: vec![name],
            });
        }
    }

    merged
}

/// Extract lines [start..end] (1-indexed, inclusive) from content.
fn extract_lines(content: &str, start: usize, end: usize) -> String {
    content
        .lines()
        .enumerate()
        .filter(|(i, _)| {
            let line_num = i + 1;
            line_num >= start && line_num <= end
        })
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_ranges_merges_overlapping() {
        let ranges = vec![
            (10, 20, "a".to_string()),
            (18, 30, "b".to_string()),
            (50, 60, "c".to_string()),
        ];
        let merged = merge_ranges(&ranges, 100);
        assert_eq!(merged.len(), 2);
        // First range: padded 5..35 merged
        assert!(merged[0].line_end >= 30);
        assert_eq!(merged[0].node_names.len(), 2);
        // Second range: padded 45..65
        assert_eq!(merged[1].node_names.len(), 1);
    }

    #[test]
    fn merge_ranges_handles_single() {
        let ranges = vec![(10, 20, "func".to_string())];
        let merged = merge_ranges(&ranges, 100);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].line_start, 5); // 10 - 5
        assert_eq!(merged[0].line_end, 25); // 20 + 5
    }

    #[test]
    fn extract_lines_works() {
        let content = "line1\nline2\nline3\nline4\nline5";
        assert_eq!(extract_lines(content, 2, 4), "line2\nline3\nline4");
    }
}
