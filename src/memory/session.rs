use rusqlite::{params, Connection};

use crate::error::Result;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub compressed: bool,
    pub summary: Option<String>,
}

pub fn ensure_session(conn: &Connection, session_id: &str) -> Result<()> {
    tracing::debug!(session_id = session_id, "Ensuring session exists");
    conn.execute(
        "INSERT INTO sessions (id) VALUES (?1)
         ON CONFLICT(id) DO UPDATE SET updated_at = datetime('now')",
        params![session_id],
    )?;
    Ok(())
}

pub fn get_session(conn: &Connection, session_id: &str) -> Result<Option<Session>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, updated_at, compressed, summary
         FROM sessions WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![session_id], |row| {
        Ok(Session {
            id: row.get(0)?,
            created_at: row.get(1)?,
            updated_at: row.get(2)?,
            compressed: row.get::<_, i32>(3)? != 0,
            summary: row.get(4)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn get_latest_session(conn: &Connection) -> Result<Option<Session>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, updated_at, compressed, summary
         FROM sessions ORDER BY updated_at DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], |row| {
        Ok(Session {
            id: row.get(0)?,
            created_at: row.get(1)?,
            updated_at: row.get(2)?,
            compressed: row.get::<_, i32>(3)? != 0,
            summary: row.get(4)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn get_previous_sessions(
    conn: &Connection,
    current_id: &str,
    limit: usize,
) -> Result<Vec<Session>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, updated_at, compressed, summary
         FROM sessions
         WHERE id != ?1
         ORDER BY updated_at DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![current_id, limit as i64], |row| {
            Ok(Session {
                id: row.get(0)?,
                created_at: row.get(1)?,
                updated_at: row.get(2)?,
                compressed: row.get::<_, i32>(3)? != 0,
                summary: row.get(4)?,
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

/// Compress stale sessions by generating extractive summaries.
/// Sessions older than `hours_threshold` that haven't been compressed get a summary.
pub fn compress_stale_sessions(conn: &Connection, hours_threshold: u64) -> Result<usize> {
    // Find sessions to compress
    let mut stmt = conn.prepare(
        "SELECT id FROM sessions
         WHERE compressed = 0
           AND updated_at < datetime('now', ?1)",
    )?;
    let threshold = format!("-{hours_threshold} hours");
    let session_ids: Vec<String> = stmt
        .query_map(params![threshold], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut compressed = 0;
    for session_id in &session_ids {
        // Get observation headlines for this session
        let mut obs_stmt = conn.prepare(
            "SELECT headline FROM observations
             WHERE session_id = ?1 AND headline IS NOT NULL
             ORDER BY created_at ASC",
        )?;
        let headlines: Vec<String> = obs_stmt
            .query_map(params![session_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        if headlines.is_empty() {
            continue;
        }

        // Build extractive summary: deduplicated headlines, capped at 500 chars
        let mut seen = std::collections::HashSet::new();
        let mut summary_parts = Vec::new();
        let mut total_len = 0;

        for h in &headlines {
            if seen.insert(h.as_str()) && total_len + h.len() + 2 <= 500 {
                summary_parts.push(h.as_str());
                total_len += h.len() + 2; // +2 for "; "
            }
        }

        let summary = summary_parts.join("; ");

        conn.execute(
            "UPDATE sessions SET compressed = 1, summary = ?1 WHERE id = ?2",
            params![summary, session_id],
        )?;
        compressed += 1;
    }

    Ok(compressed)
}
