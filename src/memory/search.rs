use rusqlite::{params, Connection};

use crate::error::Result;
use crate::graph::GraphEngine;

#[derive(Debug)]
#[allow(dead_code)]
pub struct MemorySearchResult {
    pub observation_id: i64,
    pub content: String,
    pub headline: Option<String>,
    pub summary: Option<String>,
    pub created_at: String,
    pub is_stale: bool,
    pub score: f64,
}

/// Cross-session hybrid search: FTS5 BM25 + recency decay (7-day half-life) + graph proximity.
pub fn search_observations(
    conn: &Connection,
    graph: &GraphEngine,
    query: &str,
    limit: usize,
    session_id: Option<&str>,
) -> Result<Vec<MemorySearchResult>> {
    // Sanitize query for FTS5: strip special characters that FTS5 interprets
    // as operators (hyphens, colons, quotes, etc.) to prevent parse errors.
    let fts_query = query
        .split_whitespace()
        .filter(|w| w.len() > 1)
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|w| w.len() > 1)
        .collect::<Vec<_>>()
        .join(" OR ");

    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    // FTS5 search (includes session_id to avoid N+1 queries for session bonus)
    let mut stmt = conn.prepare(
        "SELECT o.id, o.content, o.headline, o.summary, o.created_at, o.is_stale,
                observations_fts.rank,
                julianday('now') - julianday(o.created_at) as age_days,
                o.session_id
         FROM observations_fts
         JOIN observations o ON o.id = observations_fts.rowid
         WHERE observations_fts MATCH ?1
         ORDER BY observations_fts.rank
         LIMIT ?2",
    )?;

    type ObservationRow = (i64, String, Option<String>, Option<String>, String, bool, f64, f64, String);
    let raw_results: Vec<ObservationRow> = stmt
        .query_map(params![fts_query, (limit * 3) as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get::<_, i32>(5)? != 0,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if raw_results.is_empty() {
        return Ok(Vec::new());
    }

    let max_bm25 = raw_results
        .iter()
        .map(|r| r.6.abs())
        .fold(0.0_f64, f64::max);

    let mut results: Vec<MemorySearchResult> = Vec::new();

    for (id, content, headline, summary, created_at, is_stale, bm25_raw, age_days, obs_session_id) in &raw_results {
        // Normalized BM25
        let bm25_norm = if max_bm25 > 0.0 {
            bm25_raw.abs() / max_bm25
        } else {
            0.0
        };

        // Recency decay: score × 2^(-age_days/7)
        let recency = 2.0_f64.powf(-age_days / 7.0);

        // Graph proximity: check if observation is linked to nodes relevant to query
        let graph_score = compute_graph_proximity(conn, graph, *id, query);

        // Session bonus: boost if same session (uses session_id from the FTS JOIN)
        let session_bonus = if let Some(sid) = session_id {
            if obs_session_id == sid {
                0.2
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Weighted fusion
        let score = 0.4 * bm25_norm + 0.3 * recency + 0.2 * graph_score + 0.1 + session_bonus;

        results.push(MemorySearchResult {
            observation_id: *id,
            content: content.clone(),
            headline: headline.clone(),
            summary: summary.clone(),
            created_at: created_at.clone(),
            is_stale: *is_stale,
            score,
        });
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);

    Ok(results)
}

fn compute_graph_proximity(
    conn: &Connection,
    graph: &GraphEngine,
    observation_id: i64,
    _query: &str,
) -> f64 {
    // Get linked node IDs
    let linked_nodes: Vec<i64> = conn
        .prepare("SELECT node_id FROM observation_symbols WHERE observation_id = ?1")
        .and_then(|mut stmt| {
            stmt.query_map(params![observation_id], |row| row.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();

    if linked_nodes.is_empty() {
        return 0.0;
    }

    // Check if any linked nodes match query terms
    let max_pagerank = linked_nodes
        .iter()
        .map(|&id| graph.get_pagerank(id))
        .fold(0.0_f64, f64::max);

    // Normalize (PageRank values are small)
    (max_pagerank * 1000.0).min(1.0)
}
