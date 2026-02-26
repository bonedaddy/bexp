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
