use rusqlite::Connection;

use crate::db::queries;
use crate::error::Result;
use crate::graph::GraphEngine;
use crate::types::Intent;

use super::intent::intent_weights;

/// Common English stop words shared by FTS sanitization and term extraction.
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "can", "shall", "to",
    "of", "in", "for", "on", "with", "at", "by", "from", "it", "this", "that", "these", "those",
    "i", "me", "my", "we", "our", "you", "your", "he", "she", "they", "them", "what", "which",
    "who", "when", "where", "why", "how", "not", "no", "nor", "and", "or", "but", "if", "then",
];

/// Additional code-specific stop words used only in LIKE-fallback term extraction.
const CODE_STOP_WORDS: &[&str] = &[
    "implementation",
    "function",
    "method",
    "class",
    "struct",
    "code",
    "file",
    "module",
    "type",
    "interface",
    "trait",
    "find",
    "show",
    "get",
    "search",
    "look",
    "where",
    "works",
    "work",
    "use",
    "used",
    "using",
    "codebase",
    "project",
    "repository",
];

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub node_id: i64,
    pub file_id: i64,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub qualified_name: Option<String>,
    #[allow(dead_code)]
    pub kind: String,
    #[allow(dead_code)]
    pub file_path: String,
    pub score: f64,
    /// Workspace name for cross-workspace results, None for local.
    pub workspace: Option<String>,
    /// Cluster ID for grouping similar results together
    pub cluster_id: Option<String>,
}

/// Perform hybrid search combining FTS5 BM25, TF-IDF approximation, graph centrality,
/// and edge confidence. Falls back to LIKE-based search if FTS5 returns nothing.
#[allow(dead_code)]
pub fn hybrid_search(
    conn: &Connection,
    graph: &GraphEngine,
    query: &str,
    intent: &Intent,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    hybrid_search_with_external(conn, graph, query, intent, limit, None)
}

/// Like `hybrid_search` but also searches external workspace databases
/// when `external_dbs` is provided.
pub fn hybrid_search_with_external(
    conn: &Connection,
    graph: &GraphEngine,
    query: &str,
    intent: &Intent,
    limit: usize,
    external_dbs: Option<&[(String, Connection)]>,
) -> Result<Vec<SearchResult>> {
    let (bm25_weight, tfidf_weight, centrality_weight, confidence_weight) = intent_weights(intent);
    // Path bonus: additive boost for results whose file path contains query terms.
    // 0.15 is enough to meaningfully re-rank without overwhelming other signals.
    let path_weight = 0.15;

    // 1. Combined FTS5 BM25 search + node/file data in a single query
    // Strategy: try AND first for precise results, fall back to OR for broader matches.
    let fts_and_query = sanitize_fts_query_and(query);
    let fts_or_query = sanitize_fts_query(query);

    let fts_results = if fts_and_query != fts_or_query {
        let and_results =
            queries::search_nodes_fts_full(conn, &fts_and_query, limit * 2)?;
        if and_results.len() >= 5 {
            and_results
        } else {
            // AND too narrow, use OR
            queries::search_nodes_fts_full(conn, &fts_or_query, limit * 2)?
        }
    } else {
        queries::search_nodes_fts_full(conn, &fts_or_query, limit * 2)?
    };

    // 2. If FTS5 found nothing, fall back to LIKE-based search
    let (fts_results, use_like_fallback) = if fts_results.is_empty() {
        tracing::debug!(
            query = query,
            "FTS5 returned no results, falling back to LIKE search"
        );
        let like_results = fallback_like_search(conn, query, limit * 2)?;
        (Vec::new(), Some(like_results))
    } else {
        (fts_results, None)
    };

    // Edge confidence lookup: only worth the cost for small result sets
    // (the batch query is expensive for 100+ node IDs).
    // For larger sets, use default confidence to avoid ~5ms overhead.
    let node_ids: Vec<i64> = if let Some(ref like) = use_like_fallback {
        like.iter().map(|(id, _)| *id).collect()
    } else {
        fts_results.iter().map(|r| r.node_id).collect()
    };
    let confidence_map = if node_ids.len() <= 30 {
        queries::get_avg_edge_confidence_batch(conn, &node_ids)?
    } else {
        std::collections::HashMap::new() // default 0.5 via unwrap_or below
    };

    let mut results: Vec<SearchResult> = Vec::new();

    if let Some(like_results) = use_like_fallback {
        // LIKE fallback path: need to fetch node+file data separately
        let node_file_map = queries::get_nodes_with_files_by_ids(conn, &node_ids)?;
        for (node_id, bm25_raw) in &like_results {
            let nwf = match node_file_map.get(node_id) {
                Some(n) => n,
                None => continue,
            };
            let tfidf = compute_tfidf_score(query, &nwf.name, nwf.signature.as_deref());
            let centrality = graph.get_pagerank(nwf.node_id);
            let centrality_norm = (centrality * 1000.0).min(1.0);
            let confidence_norm = confidence_map.get(node_id).copied().unwrap_or(0.5);
            let path_bonus = compute_path_bonus(query, &nwf.file_path);
            let score = bm25_weight * bm25_raw.abs()
                + tfidf_weight * tfidf
                + centrality_weight * centrality_norm
                + confidence_weight * confidence_norm
                + path_weight * path_bonus;
            results.push(SearchResult {
                node_id: nwf.node_id,
                file_id: nwf.file_id,
                name: nwf.name.clone(),
                qualified_name: nwf.qualified_name.clone(),
                kind: nwf.kind.clone(),
                file_path: nwf.file_path.clone(),
                score,
                workspace: None,
                cluster_id: None,
            });
        }
    } else {
        // FTS path: data already fetched in combined query
        let max_bm25 = fts_results
            .iter()
            .map(|r| r.rank.abs())
            .fold(0.0_f64, f64::max);

        for r in &fts_results {
            let bm25_norm = if max_bm25 > 0.0 {
                r.rank.abs() / max_bm25
            } else {
                0.0
            };
            let tfidf = compute_tfidf_score(query, &r.name, r.signature.as_deref());
            let centrality = graph.get_pagerank(r.node_id);
            let centrality_norm = (centrality * 1000.0).min(1.0);
            let confidence_norm = confidence_map.get(&r.node_id).copied().unwrap_or(0.5);
            let path_bonus = compute_path_bonus(query, &r.file_path);
            let score = bm25_weight * bm25_norm
                + tfidf_weight * tfidf
                + centrality_weight * centrality_norm
                + confidence_weight * confidence_norm
                + path_weight * path_bonus;
            results.push(SearchResult {
                node_id: r.node_id,
                file_id: r.file_id,
                name: r.name.clone(),
                qualified_name: r.qualified_name.clone(),
                kind: r.kind.clone(),
                file_path: r.file_path.clone(),
                score,
                workspace: None,
                cluster_id: None,
            });
        }
    }

    // Cross-workspace search: query external DBs with 0.85x score penalty
    if let Some(ext_dbs) = external_dbs {
        let fts_query = &fts_or_query;
        for (ws_name, ext_conn) in ext_dbs {
            if let Ok(ext_results) = queries::search_nodes_fts_full(ext_conn, fts_query, limit) {
                let ext_max_bm25 = ext_results
                    .iter()
                    .map(|r| r.rank.abs())
                    .fold(0.0_f64, f64::max);

                for r in &ext_results {
                    let bm25_norm = if ext_max_bm25 > 0.0 {
                        r.rank.abs() / ext_max_bm25
                    } else {
                        0.0
                    };
                    let tfidf = compute_tfidf_score(query, &r.name, r.signature.as_deref());
                    let path_bonus = compute_path_bonus(query, &r.file_path);
                    // External results: no graph centrality or confidence (separate graph)
                    let score = (bm25_weight * bm25_norm
                        + tfidf_weight * tfidf
                        + path_weight * path_bonus)
                        * 0.85; // Cross-workspace penalty

                    results.push(SearchResult {
                        node_id: r.node_id,
                        file_id: r.file_id,
                        name: r.name.clone(),
                        qualified_name: r.qualified_name.clone(),
                        kind: r.kind.clone(),
                        file_path: r.file_path.clone(),
                        score,
                        workspace: Some(ws_name.clone()),
                        cluster_id: None,
                    });
                }
            }
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);

    // Assign cluster IDs based on directory (Heuristic 1)
    let mut dir_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in &results {
        if let Some(parent) = std::path::Path::new(&r.file_path).parent() {
            let dir = parent.to_string_lossy().to_string();
            *dir_counts.entry(dir).or_insert(0) += 1;
        }
    }
    
    for r in &mut results {
        if let Some(parent) = std::path::Path::new(&r.file_path).parent() {
            let dir = parent.to_string_lossy().to_string();
            if dir_counts.get(&dir).copied().unwrap_or(0) > 2 {
                // If more than 2 results share a directory, group them
                r.cluster_id = Some(dir);
            }
        }
    }

    Ok(results)
}

/// Fallback LIKE-based search when FTS5 returns nothing.
/// Searches node names, qualified names, file paths, and signatures
/// for any of the query terms.
fn fallback_like_search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<(i64, f64)>> {
    let terms = extract_search_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    // Build a query that matches any term against node name, qualified_name,
    // signature, or the file path
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    for term in &terms {
        let pattern = format!("%{term}%");
        conditions.push(format!(
            "(n.name LIKE ?{idx} COLLATE NOCASE \
             OR n.qualified_name LIKE ?{idx} COLLATE NOCASE \
             OR n.signature LIKE ?{idx} COLLATE NOCASE \
             OR f.path LIKE ?{idx} COLLATE NOCASE)"
        ));
        bind_values.push(Box::new(pattern));
        idx += 1;
    }

    let where_clause = conditions.join(" OR ");
    let sql = format!(
        "SELECT n.id FROM nodes n \
         JOIN files f ON f.id = n.file_id \
         WHERE ({where_clause}) \
         ORDER BY n.id \
         LIMIT ?{idx}"
    );
    bind_values.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            Ok((id, 1.0_f64)) // Uniform score for LIKE results
        })?
        .filter_map(|r| match r {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::trace!(error = %e, "Skipping row due to error");
                None
            }
        })
        .collect::<Vec<_>>();

    if rows.is_empty() {
        tracing::debug!(
            terms = ?terms,
            "LIKE fallback also returned no results"
        );
    } else {
        tracing::debug!(
            count = rows.len(),
            terms = ?terms,
            "LIKE fallback found results"
        );
    }

    Ok(rows)
}

/// Extract meaningful search terms from a natural language query.
/// Strips stop words, short words, and common noise.
fn extract_search_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|w| {
            let lower = w.to_lowercase();
            !STOP_WORDS.contains(&lower.as_str())
                && !CODE_STOP_WORDS.contains(&lower.as_str())
                && w.len() > 1
        })
        .map(|w| w.to_lowercase())
        .collect()
}

pub fn sanitize_fts_query(query: &str) -> String {
    // Convert natural language / code queries to FTS5 query.
    //
    // Strategy:
    // - For each whitespace-separated word, split on code separators (_, -, /, :, .)
    //   and camelCase boundaries to produce subterms.
    // - If a word has multiple subterms, join them with implicit AND (space-separated
    //   in parentheses) for precise compound identifier matching.
    //   e.g. "process_event" → "(process event)", "bet_placed" → "(bet placed)"
    // - Single-subterm words are used directly.
    // - All word groups are joined with OR for broad cross-word matching.
    //   e.g. "dice game logic" → "dice OR game OR logic"
    //
    // This gives precise matching for identifiers while broad matching for NL queries.
    let mut word_groups: Vec<String> = Vec::new();

    for word in query.split_whitespace() {
        let mut subterms: Vec<String> = Vec::new();

        // Split on common code separators
        for segment in word.split(['-', '/', ':', '.', '_']) {
            if segment.is_empty() {
                continue;
            }
            let cleaned: String = segment
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect();
            if cleaned.is_empty() {
                continue;
            }
            // Split camelCase/PascalCase
            let parts = split_camel_for_fts(&cleaned);
            for part in parts {
                let lower = part.to_lowercase();
                if lower.len() > 1
                    && !STOP_WORDS.contains(&lower.as_str())
                    && !CODE_STOP_WORDS.contains(&lower.as_str())
                {
                    if !subterms.contains(&lower) {
                        subterms.push(lower);
                    }
                }
            }
        }

        if subterms.is_empty() {
            continue;
        }

        if subterms.len() == 1 {
            word_groups.push(subterms.into_iter().next().unwrap());
        } else {
            // Multiple subterms from one compound word → implicit AND in parens
            word_groups.push(format!("({})", subterms.join(" ")));
        }
    }

    if word_groups.is_empty() {
        // Fallback: strip specials from original query
        let cleaned: String = query
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
            .collect();
        return cleaned;
    }

    if word_groups.len() == 1 {
        word_groups.into_iter().next().unwrap()
    } else {
        word_groups.join(" OR ")
    }
}

/// Like `sanitize_fts_query` but flattens all terms into a single implicit AND
/// (space-separated, no OR). Gives more precise results when multiple terms are present.
pub fn sanitize_fts_query_and(query: &str) -> String {
    let mut all_terms: Vec<String> = Vec::new();

    for word in query.split_whitespace() {
        for segment in word.split(['-', '/', ':', '.', '_']) {
            if segment.is_empty() {
                continue;
            }
            let cleaned: String = segment
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect();
            if cleaned.is_empty() {
                continue;
            }
            let parts = split_camel_for_fts(&cleaned);
            for part in parts {
                let lower = part.to_lowercase();
                if lower.len() > 1
                    && !STOP_WORDS.contains(&lower.as_str())
                    && !CODE_STOP_WORDS.contains(&lower.as_str())
                {
                    if !all_terms.contains(&lower) {
                        all_terms.push(lower);
                    }
                }
            }
        }
    }

    if all_terms.is_empty() {
        let cleaned: String = query
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
            .collect();
        return cleaned;
    }

    // Join with spaces = implicit AND in FTS5 (no parentheses, no OR)
    all_terms.join(" ")
}

/// Split a camelCase/PascalCase string into parts for FTS query.
fn split_camel_for_fts(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 1 {
        return vec![s.to_string()];
    }

    let mut parts = Vec::new();
    let mut current = String::new();
    current.push(chars[0]);

    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let cur = chars[i];
        let is_boundary = (prev.is_lowercase() && cur.is_uppercase())
            || (i + 1 < chars.len()
                && prev.is_uppercase()
                && cur.is_uppercase()
                && chars[i + 1].is_lowercase());
        if is_boundary {
            parts.push(current);
            current = String::new();
        }
        current.push(cur);
    }
    if !current.is_empty() {
        parts.push(current);
    }

    // Only return parts if we actually split something
    if parts.len() > 1 {
        parts
    } else {
        vec![s.to_string()]
    }
}

fn compute_tfidf_score(query: &str, name: &str, signature: Option<&str>) -> f64 {
    let query_terms: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 1 && !STOP_WORDS.contains(w))
        .map(String::from)
        .collect();

    if query_terms.is_empty() {
        return 0.0;
    }

    // Split name and signature into word tokens for boundary-aware matching
    let name_words = tokenize_identifier(name);
    let sig_words = signature.map(|s| tokenize_identifier(s)).unwrap_or_default();

    let mut matches = 0.0;
    for term in &query_terms {
        if name_words.iter().any(|w| w == term) {
            matches += 2.0; // Name match is worth more
        }
        if sig_words.iter().any(|w| w == term) {
            matches += 1.0;
        }
    }

    let total_terms = query_terms.len() as f64;
    (matches / (total_terms * 3.0)).min(1.0)
}

/// Split an identifier or signature into lowercase word tokens.
/// Handles snake_case, camelCase, PascalCase, and punctuation.
fn tokenize_identifier(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    // First split on non-alphanumeric chars (underscores, spaces, punctuation)
    for segment in s.split(|c: char| !c.is_alphanumeric()) {
        if segment.is_empty() {
            continue;
        }
        // Then split camelCase/PascalCase
        let chars: Vec<char> = segment.chars().collect();
        let mut current = String::new();
        current.push(chars[0]);

        for i in 1..chars.len() {
            let prev = chars[i - 1];
            let cur = chars[i];
            let is_boundary = (prev.is_lowercase() && cur.is_uppercase())
                || (i + 1 < chars.len()
                    && prev.is_uppercase()
                    && cur.is_uppercase()
                    && chars[i + 1].is_lowercase());
            if is_boundary {
                let lower = current.to_lowercase();
                if lower.len() > 1 {
                    words.push(lower);
                }
                current = String::new();
            }
            current.push(cur);
        }
        if !current.is_empty() {
            let lower = current.to_lowercase();
            if lower.len() > 1 {
                words.push(lower);
            }
        }
    }
    words
}

/// Compute a file path relevance bonus for a result.
/// Returns a score [0, 1] based on how many query terms appear in the file path.
fn compute_path_bonus(query: &str, file_path: &str) -> f64 {
    let terms: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 1 && !STOP_WORDS.contains(w) && !CODE_STOP_WORDS.contains(w))
        .map(String::from)
        .collect();

    if terms.is_empty() {
        return 0.0;
    }

    let path_lower = file_path.to_lowercase();
    let path_words = tokenize_identifier(&path_lower);

    let matched = terms
        .iter()
        .filter(|t| path_words.iter().any(|pw| pw == *t) || path_lower.contains(t.as_str()))
        .count();

    (matched as f64 / terms.len() as f64).min(1.0)
}
