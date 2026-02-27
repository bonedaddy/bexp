use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::search::SearchResult;
use crate::config::BexpConfig;
use crate::db::queries;
use crate::error::Result;
use crate::graph::GraphEngine;
use crate::skeleton::Skeletonizer;
use crate::types::{DetailLevel, Intent, Language};

#[derive(Debug)]
pub struct BudgetAllocation {
    /// Excerpt pivots (full file or node-level excerpts)
    pub pivots: Vec<PivotExcerpt>,
    /// Bridge context (signature-only from graph neighbors)
    pub bridges: Vec<BridgeExcerpt>,
    /// Files to include as skeletons
    pub skeletons: Vec<SkeletonFile>,
    /// Collapsed summary of matching files grouped by cluster
    pub rollups: Vec<ClusterRollup>,
    /// Files that matched but were dropped due to budget constraints
    pub unallocated_matches: Vec<UnallocatedMatch>,
    /// Total tokens used
    pub total_tokens: usize,
}

#[derive(Debug)]
pub struct ClusterRollup {
    pub cluster_name: String,
    pub representative_file: String,
    pub matched_siblings: Vec<(String, String)>, // (file_path, matching_node_name)
}

#[derive(Debug)]
pub struct UnallocatedMatch {
    pub path: String,
    pub score: f64,
}

#[derive(Debug)]
pub struct PivotExcerpt {
    #[allow(dead_code)]
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
pub struct BridgeExcerpt {
    #[allow(dead_code)]
    pub file_id: i64,
    pub path: String,
    pub signature: String,
    #[allow(dead_code)]
    pub tokens: usize,
    pub node_name: String,
}

#[derive(Debug)]
pub struct SkeletonFile {
    #[allow(dead_code)]
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

/// Greedy budget allocation with node-level granularity.
///
/// Budget split is controlled by `BexpConfig` fields:
/// `overhead_reserve_pct` is reserved first, then the usable remainder is split
/// into `pivot_budget_pct` for pivots, `bridge_budget_pct` for bridges,
/// and the remainder for skeletons.
pub fn allocate(
    conn: &Connection,
    skeletonizer: &Skeletonizer,
    search_results: &[SearchResult],
    budget: usize,
    intent: &Intent,
    graph: Option<&GraphEngine>,
    config: &BexpConfig,
) -> Result<BudgetAllocation> {
    let mut allocation = BudgetAllocation {
        pivots: Vec::new(),
        bridges: Vec::new(),
        skeletons: Vec::new(),
        rollups: Vec::new(),
        unallocated_matches: Vec::new(),
        total_tokens: 0,
    };

    // Reserve overhead budget and split remainder using integer arithmetic
    // for deterministic rounding.
    let usable_budget = budget * (100 - config.overhead_reserve_pct) / 100;
    let pivot_budget = usable_budget * config.pivot_budget_pct / 100;
    let bridge_budget = usable_budget * config.bridge_budget_pct / 100;
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
    let mut file_results: HashMap<i64, &SearchResult> = HashMap::new();
    for result in search_results {
        let current_score = file_scores.entry(result.file_id).or_insert(result.score);
        if result.score >= *current_score {
            *current_score = result.score;
            file_results.insert(result.file_id, result);
        }
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

    // Batch-fetch node counts for all candidate files
    let file_node_counts =
        queries::get_file_node_counts_batch(conn, &file_order).unwrap_or_default();

    let mut seen_files = HashSet::new();
    let mut included_node_ids: HashSet<i64> = HashSet::new();

    // Build excerpts for top files
    for &file_id in file_order.iter().take(config.max_pivot_files) {
        if pivot_remaining < config.min_pivot_budget {
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

        // Get total node count to decide full-file vs excerpt strategy
        let total_node_count = file_node_counts.get(&file_id).copied().unwrap_or(0) as usize;
        let matched_count = ranges.len();

        // Read file content
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let total_lines = content.lines().count();

        // Phase 3: Sniper Snippets for Usage Intent
        let is_usage_intent = matches!(intent, Intent::BlastRadius);
        let padding = if is_usage_intent {
            config.context_padding.min(2) // Tighter padding for snippets
        } else {
            config.context_padding
        };

        // Decide: full file or excerpt
        let use_full_file = if is_usage_intent {
            false // Force snippets for usage discovery
        } else {
            total_lines < 50 || (total_node_count > 0 && matched_count * 2 >= total_node_count)
        };

        if use_full_file {
            let tokens = skeletonizer.count_tokens_fast(&content);
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
            let merged = merge_ranges(&raw_ranges, total_lines, padding);

            for mr in merged {
                let excerpt = extract_lines(&content, mr.line_start, mr.line_end);
                let tokens = skeletonizer.count_tokens_fast(&excerpt);
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
            if bridge_remaining < config.min_bridge_budget {
                break;
            }
            let sig = nr.signature.as_deref().unwrap_or(&nr.name);
            let tokens = skeletonizer.count_tokens_fast(sig);
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

    // Phase 1: Clusters & Rollups
    let mut seen_clusters: HashMap<String, String> = HashMap::new();
    for pivot in &allocation.pivots {
        if let Some(r) = file_results.get(&pivot.file_id) {
            if let Some(cid) = &r.cluster_id {
                seen_clusters.insert(cid.clone(), r.file_path.clone());
            }
        }
    }

    let mut skeleton_files: Vec<(i64, f64)> = Vec::new();
    let mut skel_seen = HashSet::new();
    let mut cluster_rollups_map: HashMap<String, ClusterRollup> = HashMap::new();

    for result in search_results {
        if seen_files.contains(&result.file_id) {
            continue;
        }

        if let Some(cid) = &result.cluster_id {
            if let Some(rep_file) = seen_clusters.get(cid) {
                // We already have a representative for this cluster
                let rollup =
                    cluster_rollups_map
                        .entry(cid.clone())
                        .or_insert_with(|| ClusterRollup {
                            cluster_name: cid.clone(),
                            representative_file: rep_file.clone(),
                            matched_siblings: Vec::new(),
                        });
                rollup
                    .matched_siblings
                    .push((result.file_path.clone(), result.name.clone()));
                seen_files.insert(result.file_id); // Mark as seen so we skip skeleton
                continue;
            } else {
                seen_clusters.insert(cid.clone(), result.file_path.clone());
            }
        }

        if skel_seen.insert(result.file_id) {
            skeleton_files.push((result.file_id, result.score));
        }
    }

    // Phase 2: Dynamic Level-of-Detail
    let significant_files = file_scores
        .values()
        .filter(|&&s| s > high_threshold)
        .count();
    let dynamic_preferred_level = match significant_files {
        0..=3 => DetailLevel::Detailed,
        4..=10 => DetailLevel::Standard,
        _ => DetailLevel::Minimal,
    };

    // Pre-fetch skeleton data for candidate files in one batch query
    let skel_file_ids: Vec<i64> = skeleton_files
        .iter()
        .take(config.max_skeleton_files * 2)
        .map(|(id, _)| *id)
        .collect();
    let skel_cache = queries::get_skeleton_cache_batch(conn, &skel_file_ids).unwrap_or_default();

    for (file_id, score) in skeleton_files {
        if skeleton_remaining < config.min_skeleton_budget
            || allocation.skeletons.len() >= config.max_skeleton_files
        {
            if let Some(r) = file_results.get(&file_id) {
                allocation.unallocated_matches.push(UnallocatedMatch {
                    path: r.file_path.clone(),
                    score,
                });
            }
            continue;
        }

        let cached_row = match skel_cache.get(&file_id) {
            Some(r) => r,
            None => {
                if let Some(r) = file_results.get(&file_id) {
                    allocation.unallocated_matches.push(UnallocatedMatch {
                        path: r.file_path.clone(),
                        score,
                    });
                }
                continue;
            }
        };

        let ext = std::path::Path::new(&cached_row.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let lang = match Language::from_extension(ext) {
            Some(l) => l,
            None => {
                if let Some(r) = file_results.get(&file_id) {
                    allocation.unallocated_matches.push(UnallocatedMatch {
                        path: r.file_path.clone(),
                        score,
                    });
                }
                continue;
            }
        };

        let levels_to_try = match dynamic_preferred_level {
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
            let cached = match level {
                DetailLevel::Minimal => cached_row.skeleton_minimal.as_ref(),
                DetailLevel::Standard => cached_row.skeleton_standard.as_ref(),
                DetailLevel::Detailed => cached_row.skeleton_detailed.as_ref(),
            };

            let skel = if let Some(cached_skel) = cached {
                cached_skel.clone()
            } else {
                let content = match std::fs::read_to_string(&cached_row.path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                skeletonizer
                    .skeletonize_source(&content, lang, level)
                    .unwrap_or_default()
            };

            let tok = skeletonizer.count_tokens_fast(&skel);
            if tok <= skeleton_remaining {
                chosen = Some((skel, tok, level));
                break;
            }
        }

        let (skeleton, tokens, level) = match chosen {
            Some(c) => c,
            None => {
                if let Some(r) = file_results.get(&file_id) {
                    allocation.unallocated_matches.push(UnallocatedMatch {
                        path: r.file_path.clone(),
                        score,
                    });
                }
                continue;
            }
        };

        skeleton_remaining -= tokens;
        allocation.skeletons.push(SkeletonFile {
            file_id,
            path: cached_row.path.clone(),
            skeleton,
            tokens,
            level,
        });
    }

    allocation.rollups = cluster_rollups_map.into_values().collect();

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
fn merge_ranges(
    ranges: &[(usize, usize, String)],
    total_lines: usize,
    context_padding: usize,
) -> Vec<MergedRange> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut padded: Vec<(usize, usize, String)> = ranges
        .iter()
        .map(|(start, end, name)| {
            let padded_start = start.saturating_sub(context_padding).max(1);
            let padded_end = (*end + context_padding).min(total_lines);
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
        let merged = merge_ranges(&ranges, 100, 5);
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
        let merged = merge_ranges(&ranges, 100, 5);
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
