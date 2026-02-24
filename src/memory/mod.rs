pub mod observation;
pub mod search;
pub mod session;

use std::sync::Arc;

use crate::db::Database;
use crate::error::Result;
use crate::graph::GraphEngine;

pub struct MemoryService {
    db: Arc<Database>,
    graph: Arc<GraphEngine>,
}

impl MemoryService {
    pub fn new(db: Arc<Database>, graph: Arc<GraphEngine>) -> Self {
        Self { db, graph }
    }

    pub fn get_session_context(
        &self,
        session_id: Option<&str>,
        include_previous: bool,
        previous_limit: usize,
    ) -> Result<String> {
        let conn = &*self.db.reader();

        let current_session = match session_id {
            Some(id) => session::get_session(conn, id)?,
            None => session::get_latest_session(conn)?,
        };

        let current_session = match current_session {
            Some(s) => s,
            None => return Ok("No active session found.".to_string()),
        };

        let mut output = String::new();
        output.push_str(&format!("# Session: {}\n\n", current_session.id));
        output.push_str(&format!(
            "**Created:** {}\n**Updated:** {}\n\n",
            current_session.created_at, current_session.updated_at
        ));

        // Get observations for current session
        let observations = observation::get_observations_for_session(conn, &current_session.id)?;

        if observations.is_empty() {
            output.push_str("No observations in this session.\n");
        } else {
            output.push_str("## Observations\n\n");
            for obs in &observations {
                let stale_marker = if obs.is_stale { " [STALE]" } else { "" };
                output.push_str(&format!(
                    "### {}{}\n\n{}\n\n*{}*\n\n",
                    obs.headline.as_deref().unwrap_or("Observation"),
                    stale_marker,
                    obs.content,
                    obs.created_at,
                ));
                if obs.is_stale {
                    if let Some(reason) = &obs.stale_reason {
                        output.push_str(&format!("**Stale reason:** {}\n\n", reason));
                    }
                }
            }
        }

        // Include previous sessions if requested
        if include_previous {
            let previous = session::get_previous_sessions(conn, &current_session.id, previous_limit)?;
            if !previous.is_empty() {
                output.push_str("\n---\n\n## Previous Sessions\n\n");
                for prev in &previous {
                    output.push_str(&format!("### Session {} ({})\n\n", prev.id, prev.created_at));
                    if let Some(summary) = &prev.summary {
                        output.push_str(&format!("{}\n\n", summary));
                    }
                    let obs = observation::get_observations_for_session(conn, &prev.id)?;
                    for o in obs.iter().take(5) {
                        output.push_str(&format!(
                            "- **{}**: {}\n",
                            o.headline.as_deref().unwrap_or("Note"),
                            o.summary.as_deref().unwrap_or(&o.content)
                        ));
                    }
                    output.push('\n');
                }
            }
        }

        Ok(output)
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
    ) -> Result<String> {
        let conn = &*self.db.reader();
        let results = search::search_observations(conn, &self.graph, query, limit, session_id)?;

        if results.is_empty() {
            return Ok(String::new());
        }

        let mut output = String::new();
        for result in &results {
            let stale = if result.is_stale { " [STALE]" } else { "" };
            output.push_str(&format!(
                "- **{}**{} (score: {:.2}, {})\n  {}\n\n",
                result.headline.as_deref().unwrap_or("Observation"),
                stale,
                result.score,
                result.created_at,
                result.summary.as_deref().unwrap_or(&result.content),
            ));
        }

        Ok(output)
    }

    pub fn save_observation(
        &self,
        session_id: &str,
        content: &str,
        symbols: Option<&[String]>,
        files: Option<&[String]>,
    ) -> Result<String> {
        let conn = self.db.writer();

        // Ensure session exists
        session::ensure_session(&conn, session_id)?;

        // Generate headline (first line or truncated)
        let headline = generate_headline(content);
        let summary = generate_summary(content);

        let obs_id = observation::insert_observation(
            &conn,
            session_id,
            content,
            Some(&headline),
            Some(&summary),
        )?;

        // Link to symbols
        if let Some(symbol_names) = symbols {
            for name in symbol_names {
                if let Some(node_id) = find_node_by_name(&conn, name) {
                    observation::link_observation_symbol(&conn, obs_id, node_id)?;
                }
            }
        }

        // Link to files
        if let Some(file_paths) = files {
            for path in file_paths {
                if let Ok(Some(file)) = crate::db::queries::get_file_by_path(&conn, path) {
                    observation::link_observation_file(&conn, obs_id, file.id, &file.content_hash)?;
                }
            }
        }

        Ok(format!(
            "Observation saved (id: {}).\n**Headline:** {}",
            obs_id, headline
        ))
    }
}

fn generate_headline(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or(content);
    if first_line.len() <= 80 {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..77])
    }
}

fn generate_summary(content: &str) -> String {
    if content.len() <= 200 {
        content.to_string()
    } else {
        format!("{}...", &content[..197])
    }
}

fn find_node_by_name(conn: &rusqlite::Connection, name: &str) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM nodes WHERE name = ?1 OR qualified_name = ?1 LIMIT 1",
        rusqlite::params![name],
        |row| row.get(0),
    )
    .ok()
}
