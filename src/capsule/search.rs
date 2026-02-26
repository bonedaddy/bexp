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
}

/// Perform hybrid search combining FTS5 BM25, TF-IDF approximation, graph centrality,
/// and edge confidence. Falls back to LIKE-based search if FTS5 returns nothing.
pub fn hybrid_search(
    conn: &Connection,
    graph: &GraphEngine,
    query: &str,
    intent: &Intent,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let (bm25_weight, tfidf_weight, centrality_weight, confidence_weight) = intent_weights(intent);

    // 1. Combined FTS5 BM25 search + node/file data in a single query
    let fts_query = sanitize_fts_query(query);
    let fts_results = queries::search_nodes_fts_full(conn, &fts_query, limit * 2)?;

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

    // Batch-fetch edge confidence for all result node IDs
    let node_ids: Vec<i64> = if let Some(ref like) = use_like_fallback {
        like.iter().map(|(id, _)| *id).collect()
    } else {
        fts_results.iter().map(|r| r.node_id).collect()
    };
    let confidence_map = queries::get_avg_edge_confidence_batch(conn, &node_ids)?;

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
            let score = bm25_weight * bm25_raw.abs()
                + tfidf_weight * tfidf
                + centrality_weight * centrality_norm
                + confidence_weight * confidence_norm;
            results.push(SearchResult {
                node_id: nwf.node_id,
                file_id: nwf.file_id,
                name: nwf.name.clone(),
                qualified_name: nwf.qualified_name.clone(),
                kind: nwf.kind.clone(),
                file_path: nwf.file_path.clone(),
                score,
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
            let score = bm25_weight * bm25_norm
                + tfidf_weight * tfidf
                + centrality_weight * centrality_norm
                + confidence_weight * confidence_norm;
            results.push(SearchResult {
                node_id: r.node_id,
                file_id: r.file_id,
                name: r.name.clone(),
                qualified_name: r.qualified_name.clone(),
                kind: r.kind.clone(),
                file_path: r.file_path.clone(),
                score,
            });
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);

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
                if lower.len() > 1 && !STOP_WORDS.contains(&lower.as_str()) {
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
        .map(String::from)
        .collect();

    let name_lower = name.to_lowercase();
    let sig_lower = signature.map(|s| s.to_lowercase()).unwrap_or_default();

    let mut matches = 0.0;
    for term in &query_terms {
        if name_lower.contains(term.as_str()) {
            matches += 2.0; // Name match is worth more
        }
        if sig_lower.contains(term.as_str()) {
            matches += 1.0;
        }
    }

    let total_terms = query_terms.len() as f64;
    if total_terms > 0.0 {
        (matches / (total_terms * 3.0)).min(1.0)
    } else {
        0.0
    }
}
