use rusqlite::Connection;

use crate::db::queries;
use crate::error::Result;
use crate::graph::GraphEngine;
use crate::types::Intent;

use super::intent::intent_weights;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub node_id: i64,
    pub file_id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: String,
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
    let (bm25_weight, tfidf_weight, centrality_weight, confidence_weight) =
        intent_weights(intent);

    // 1. FTS5 BM25 search
    let fts_query = sanitize_fts_query(query);
    let fts_results = queries::search_nodes_fts(conn, &fts_query, limit * 2)?;

    // 2. If FTS5 found nothing, fall back to LIKE-based search on names, qualified names,
    //    file paths, and signatures
    let fts_results = if fts_results.is_empty() {
        tracing::debug!("FTS5 returned no results for '{}', falling back to LIKE search", query);
        fallback_like_search(conn, query, limit * 2)?
    } else {
        fts_results
    };

    // Normalize BM25 scores
    let max_bm25 = fts_results
        .iter()
        .map(|(_, score)| score.abs())
        .fold(0.0_f64, f64::max);

    // Batch-query confidence for all result node IDs
    let node_ids: Vec<i64> = fts_results.iter().map(|(id, _)| *id).collect();
    let confidence_map = queries::get_avg_edge_confidence_batch(conn, &node_ids)?;

    let mut results: Vec<SearchResult> = Vec::new();

    for (node_id, bm25_raw) in &fts_results {
        let node = match queries::get_node_by_id(conn, *node_id)? {
            Some(n) => n,
            None => continue,
        };

        let file = match queries::get_file_by_id(conn, node.file_id)? {
            Some(f) => f,
            None => continue,
        };

        // Normalized BM25 (inverted since FTS5 returns negative ranks)
        let bm25_norm = if max_bm25 > 0.0 {
            bm25_raw.abs() / max_bm25
        } else {
            0.0
        };

        // TF-IDF approximation: simple term frequency in the name/signature
        let tfidf = compute_tfidf_score(query, &node.name, node.signature.as_deref());

        // Graph centrality (PageRank)
        let centrality = graph.get_pagerank(node.id);

        // Normalize centrality to 0-1 range (PageRank values are typically small)
        let centrality_norm = (centrality * 1000.0).min(1.0);

        // Edge confidence (0.5 default if no edges)
        let confidence_norm = confidence_map.get(node_id).copied().unwrap_or(0.5);

        // Weighted fusion
        let score = bm25_weight * bm25_norm
            + tfidf_weight * tfidf
            + centrality_weight * centrality_norm
            + confidence_weight * confidence_norm;

        results.push(SearchResult {
            node_id: node.id,
            file_id: node.file_id,
            name: node.name,
            qualified_name: node.qualified_name,
            kind: node.kind,
            file_path: file.path,
            score,
        });
    }

    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);

    Ok(results)
}

/// Fallback LIKE-based search when FTS5 returns nothing.
/// Searches node names, qualified names, file paths, and signatures
/// for any of the query terms.
fn fallback_like_search(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<(i64, f64)>> {
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
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    if rows.is_empty() {
        tracing::debug!("LIKE fallback also returned no results for terms: {:?}", terms);
    } else {
        tracing::debug!("LIKE fallback found {} results for terms: {:?}", rows.len(), terms);
    }

    Ok(rows)
}

/// Extract meaningful search terms from a natural language query.
/// Strips stop words, short words, and common noise.
fn extract_search_terms(query: &str) -> Vec<String> {
    let stop_words = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been",
        "being", "have", "has", "had", "do", "does", "did", "will",
        "would", "could", "should", "may", "might", "can", "shall",
        "to", "of", "in", "for", "on", "with", "at", "by", "from",
        "it", "this", "that", "these", "those", "i", "me", "my",
        "we", "our", "you", "your", "he", "she", "they", "them",
        "what", "which", "who", "when", "where", "why", "how",
        "not", "no", "nor", "and", "or", "but", "if", "then",
        "implementation", "function", "method", "class", "struct",
        "code", "file", "module", "type", "interface", "trait",
        "find", "show", "get", "search", "look", "where",
    ];

    query
        .split_whitespace()
        .filter(|w| {
            let lower = w.to_lowercase();
            !stop_words.contains(&lower.as_str()) && w.len() > 1
        })
        .map(|w| w.to_lowercase())
        .collect()
}

fn sanitize_fts_query(query: &str) -> String {
    // Convert natural language query to FTS5 query
    // Remove stop words and special characters, join with OR
    let stop_words = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been",
        "being", "have", "has", "had", "do", "does", "did", "will",
        "would", "could", "should", "may", "might", "can", "shall",
        "to", "of", "in", "for", "on", "with", "at", "by", "from",
        "it", "this", "that", "these", "those", "i", "me", "my",
        "we", "our", "you", "your", "he", "she", "they", "them",
        "what", "which", "who", "when", "where", "why", "how",
        "not", "no", "nor", "and", "or", "but", "if", "then",
    ];

    let tokens: Vec<&str> = query
        .split_whitespace()
        .filter(|w| {
            let lower = w.to_lowercase();
            !stop_words.contains(&lower.as_str()) && w.len() > 1
        })
        .collect();

    if tokens.is_empty() {
        return query.to_string();
    }

    tokens.join(" OR ")
}

fn compute_tfidf_score(query: &str, name: &str, signature: Option<&str>) -> f64 {
    let query_terms: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();

    let name_lower = name.to_lowercase();
    let sig_lower = signature
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

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
