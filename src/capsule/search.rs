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

/// Perform hybrid search combining FTS5 BM25, TF-IDF approximation, and graph centrality.
pub fn hybrid_search(
    conn: &Connection,
    graph: &GraphEngine,
    query: &str,
    intent: &Intent,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let (bm25_weight, tfidf_weight, centrality_weight) = intent_weights(intent);

    // 1. FTS5 BM25 search
    let fts_query = sanitize_fts_query(query);
    let fts_results = queries::search_nodes_fts(conn, &fts_query, limit * 2)?;

    // Normalize BM25 scores
    let max_bm25 = fts_results
        .iter()
        .map(|(_, score)| score.abs())
        .fold(0.0_f64, f64::max);

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

        // Weighted fusion
        let score = bm25_weight * bm25_norm
            + tfidf_weight * tfidf
            + centrality_weight * centrality_norm;

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
