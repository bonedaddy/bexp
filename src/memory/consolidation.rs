use std::collections::HashSet;

use rusqlite::{params, Connection};

use crate::error::Result;

/// Character trigrams from lowercase text.
fn trigrams(text: &str) -> HashSet<String> {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut set = HashSet::new();
    if chars.len() < 3 {
        set.insert(lower);
        return set;
    }
    for window in chars.windows(3) {
        set.insert(window.iter().collect());
    }
    set
}

/// Jaccard similarity between two sets.
fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Get linked symbol node_ids for an observation.
fn get_linked_symbol_ids(conn: &Connection, obs_id: i64) -> HashSet<i64> {
    conn.prepare("SELECT node_id FROM observation_symbols WHERE observation_id = ?1")
        .and_then(|mut stmt| {
            stmt.query_map(params![obs_id], |row| row.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
}

/// Get linked file_ids for an observation.
fn get_linked_file_ids(conn: &Connection, obs_id: i64) -> HashSet<i64> {
    conn.prepare("SELECT file_id FROM observation_files WHERE observation_id = ?1")
        .and_then(|mut stmt| {
            stmt.query_map(params![obs_id], |row| row.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
}

/// Compute similarity between two observations.
/// Weighted fusion: 0.3 symbol overlap + 0.3 file overlap + 0.4 content trigram similarity.
fn compute_similarity(
    conn: &Connection,
    obs_a_id: i64,
    obs_a_content: &str,
    obs_b_id: i64,
    obs_b_content: &str,
) -> f64 {
    let symbols_a = get_linked_symbol_ids(conn, obs_a_id);
    let symbols_b = get_linked_symbol_ids(conn, obs_b_id);

    let files_a = get_linked_file_ids(conn, obs_a_id);
    let files_b = get_linked_file_ids(conn, obs_b_id);

    let has_symbols = !symbols_a.is_empty() || !symbols_b.is_empty();
    let has_files = !files_a.is_empty() || !files_b.is_empty();

    let symbol_sim = if has_symbols {
        let inter = symbols_a.intersection(&symbols_b).count() as f64;
        let union = symbols_a.union(&symbols_b).count() as f64;
        if union > 0.0 {
            inter / union
        } else {
            0.0
        }
    } else {
        0.0
    };

    let file_sim = if has_files {
        let inter = files_a.intersection(&files_b).count() as f64;
        let union = files_a.union(&files_b).count() as f64;
        if union > 0.0 {
            inter / union
        } else {
            0.0
        }
    } else {
        0.0
    };

    let trig_a = trigrams(obs_a_content);
    let trig_b = trigrams(obs_b_content);
    let content_sim = jaccard(&trig_a, &trig_b);

    // Adjust weights based on available signals.
    // When there are no linked symbols/files, content similarity is primary signal.
    if !has_symbols && !has_files {
        content_sim // pure content similarity
    } else {
        0.3 * symbol_sim + 0.3 * file_sim + 0.4 * content_sim
    }
}

/// Check if a newly saved observation is similar to existing ones
/// and consolidate (mark older ones as superseded).
/// Returns the number of observations consolidated.
pub fn check_and_consolidate(
    conn: &Connection,
    new_obs_id: i64,
    session_id: &str,
) -> Result<usize> {
    // Get the new observation's content
    let new_content: String = conn.query_row(
        "SELECT content FROM observations WHERE id = ?1",
        params![new_obs_id],
        |row| row.get(0),
    )?;

    let mut consolidated = 0;

    // Fetch recent same-session observations (not already consolidated)
    let mut stmt = conn.prepare(
        "SELECT id, content FROM observations
         WHERE session_id = ?1 AND id != ?2
           AND consolidated_into IS NULL AND is_stale = 0
         ORDER BY created_at DESC LIMIT 20",
    )?;
    let same_session: Vec<(i64, String)> = stmt
        .query_map(params![session_id, new_obs_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Fetch recent cross-session observations
    let mut cross_stmt = conn.prepare(
        "SELECT id, content FROM observations
         WHERE session_id != ?1 AND id != ?2
           AND consolidated_into IS NULL AND is_stale = 0
         ORDER BY created_at DESC LIMIT 10",
    )?;
    let cross_session: Vec<(i64, String)> = cross_stmt
        .query_map(params![session_id, new_obs_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let candidates: Vec<(i64, String)> = same_session.into_iter().chain(cross_session).collect();

    for (old_id, old_content) in &candidates {
        let sim = compute_similarity(conn, new_obs_id, &new_content, *old_id, old_content);
        if sim >= 0.7 {
            conn.execute(
                "UPDATE observations SET consolidated_into = ?1, is_stale = 1, stale_reason = 'consolidated'
                 WHERE id = ?2",
                params![new_obs_id, old_id],
            )?;
            consolidated += 1;
        }
    }

    Ok(consolidated)
}

/// Detect anti-patterns in a session's observations.
/// Returns the number of observations tagged.
pub fn detect_anti_patterns(conn: &Connection, session_id: &str) -> Result<usize> {
    let mut tagged = 0;

    // file_thrashing: same file_id in >3 observations in session
    let thrashing_obs: Vec<i64> = conn
        .prepare(
            "SELECT DISTINCT of2.observation_id
             FROM observation_files of2
             JOIN observations o ON o.id = of2.observation_id
             WHERE o.session_id = ?1 AND o.anti_pattern IS NULL
             GROUP BY of2.file_id, of2.observation_id
             HAVING (SELECT COUNT(*) FROM observation_files of3
                     JOIN observations o2 ON o2.id = of3.observation_id
                     WHERE o2.session_id = ?1 AND of3.file_id = of2.file_id) > 3",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![session_id], |row| row.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();

    for obs_id in &thrashing_obs {
        conn.execute(
            "UPDATE observations SET anti_pattern = 'file_thrashing' WHERE id = ?1 AND anti_pattern IS NULL",
            params![obs_id],
        )?;
        tagged += 1;
    }

    // circular_investigation: same node_id in >3 observations in session
    let circular_obs: Vec<i64> = conn
        .prepare(
            "SELECT DISTINCT os.observation_id
             FROM observation_symbols os
             JOIN observations o ON o.id = os.observation_id
             WHERE o.session_id = ?1 AND o.anti_pattern IS NULL
             GROUP BY os.node_id, os.observation_id
             HAVING (SELECT COUNT(*) FROM observation_symbols os2
                     JOIN observations o2 ON o2.id = os2.observation_id
                     WHERE o2.session_id = ?1 AND os2.node_id = os.node_id) > 3",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![session_id], |row| row.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();

    for obs_id in &circular_obs {
        conn.execute(
            "UPDATE observations SET anti_pattern = 'circular_investigation' WHERE id = ?1 AND anti_pattern IS NULL",
            params![obs_id],
        )?;
        tagged += 1;
    }

    // rapid_churn: >5 observations in 10-minute window
    let rapid_obs: Vec<i64> = conn
        .prepare(
            "SELECT o.id FROM observations o
             WHERE o.session_id = ?1 AND o.anti_pattern IS NULL
               AND (SELECT COUNT(*) FROM observations o2
                    WHERE o2.session_id = ?1
                      AND ABS(julianday(o2.created_at) - julianday(o.created_at)) < (10.0/1440.0)) > 5",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![session_id], |row| row.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();

    for obs_id in &rapid_obs {
        conn.execute(
            "UPDATE observations SET anti_pattern = 'rapid_churn' WHERE id = ?1 AND anti_pattern IS NULL",
            params![obs_id],
        )?;
        tagged += 1;
    }

    Ok(tagged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigram_basic() {
        let t = trigrams("hello");
        assert!(t.contains("hel"));
        assert!(t.contains("ell"));
        assert!(t.contains("llo"));
    }

    #[test]
    fn jaccard_identical() {
        let a = trigrams("hello world");
        let b = trigrams("hello world");
        assert!((jaccard(&a, &b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_different() {
        let a = trigrams("hello");
        let b = trigrams("xyz");
        assert!(jaccard(&a, &b) < 0.1);
    }

    #[test]
    fn consolidation_with_similar_obs() {
        use crate::db::Database;
        use crate::memory::{observation, session};

        let db = Database::open_test().unwrap();
        let conn = db.writer().unwrap();

        session::ensure_session(&conn, "test-session").unwrap();

        // Insert two similar observations
        let obs1 = observation::insert_observation(
            &conn,
            "test-session",
            "The DiceRoll function handles random number generation for the game",
            Some("DiceRoll function"),
            Some("DiceRoll handles RNG"),
        )
        .unwrap();

        let obs2 = observation::insert_observation(
            &conn,
            "test-session",
            "The DiceRoll function handles random number generation for the game engine",
            Some("DiceRoll function"),
            Some("DiceRoll handles RNG for engine"),
        )
        .unwrap();

        let consolidated = check_and_consolidate(&conn, obs2, "test-session").unwrap();
        assert_eq!(consolidated, 1);

        // Verify obs1 is marked as consolidated into obs2
        let is_stale: bool = conn
            .query_row(
                "SELECT is_stale FROM observations WHERE id = ?1",
                params![obs1],
                |row| Ok(row.get::<_, i32>(0)? != 0),
            )
            .unwrap();
        assert!(is_stale);

        let consolidated_into: Option<i64> = conn
            .query_row(
                "SELECT consolidated_into FROM observations WHERE id = ?1",
                params![obs1],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(consolidated_into, Some(obs2));
    }
}
