use rusqlite::{params, Connection};

use crate::error::Result;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Observation {
    pub id: i64,
    pub session_id: String,
    pub content: String,
    pub headline: Option<String>,
    pub summary: Option<String>,
    pub created_at: String,
    pub is_stale: bool,
    pub stale_reason: Option<String>,
}

pub fn insert_observation(
    conn: &Connection,
    session_id: &str,
    content: &str,
    headline: Option<&str>,
    summary: Option<&str>,
) -> Result<i64> {
    tracing::debug!(session_id = session_id, content_len = content.len(), "Inserting observation");
    conn.execute(
        "INSERT INTO observations (session_id, content, headline, summary)
         VALUES (?1, ?2, ?3, ?4)",
        params![session_id, content, headline, summary],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_observations_for_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<Observation>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, content, headline, summary, created_at, is_stale, stale_reason
         FROM observations
         WHERE session_id = ?1
         ORDER BY created_at ASC",
    )?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(Observation {
                id: row.get(0)?,
                session_id: row.get(1)?,
                content: row.get(2)?,
                headline: row.get(3)?,
                summary: row.get(4)?,
                created_at: row.get(5)?,
                is_stale: row.get::<_, i32>(6)? != 0,
                stale_reason: row.get(7)?,
            })
        })?
        .filter_map(|r| match r {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::trace!(error = %e, "Skipping row due to error");
                None
            }
        })
        .collect();
    Ok(rows)
}

pub fn link_observation_symbol(conn: &Connection, observation_id: i64, node_id: i64) -> Result<()> {
    tracing::trace!(observation_id = observation_id, node_id = node_id, "Linking observation to symbol");
    conn.execute(
        "INSERT OR IGNORE INTO observation_symbols (observation_id, node_id)
         VALUES (?1, ?2)",
        params![observation_id, node_id],
    )?;
    Ok(())
}

pub fn link_observation_file(
    conn: &Connection,
    observation_id: i64,
    file_id: i64,
    content_hash: &str,
) -> Result<()> {
    tracing::trace!(observation_id = observation_id, file_id = file_id, "Linking observation to file");
    conn.execute(
        "INSERT OR IGNORE INTO observation_files (observation_id, file_id, content_hash_at_link)
         VALUES (?1, ?2, ?3)",
        params![observation_id, file_id, content_hash],
    )?;
    Ok(())
}

/// Check for stale observations: where linked files have changed their content hash.
pub fn detect_staleness(conn: &Connection) -> Result<usize> {
    let updated = conn.execute(
        "UPDATE observations SET is_stale = 1, stale_reason = 'linked file changed'
         WHERE id IN (
             SELECT DISTINCT of2.observation_id
             FROM observation_files of2
             JOIN files f ON f.id = of2.file_id
             WHERE f.content_hash != of2.content_hash_at_link
         ) AND is_stale = 0",
        [],
    )?;
    Ok(updated)
}

/// Delete old observations beyond the TTL.
pub fn cleanup_old_observations(conn: &Connection, ttl_days: u64) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM observations
         WHERE created_at < datetime('now', ?1)",
        params![format!("-{} days", ttl_days)],
    )?;
    Ok(deleted)
}
