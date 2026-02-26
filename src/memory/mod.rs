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
        tracing::debug!(
            session_id = ?session_id,
            include_previous = include_previous,
            "Loading session context"
        );
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
                        output.push_str(&format!("**Stale reason:** {reason}\n\n"));
                    }
                }
            }
        }

        // Include previous sessions if requested
        if include_previous {
            let previous =
                session::get_previous_sessions(conn, &current_session.id, previous_limit)?;
            if !previous.is_empty() {
                output.push_str("\n---\n\n## Previous Sessions\n\n");
                for prev in &previous {
                    output.push_str(&format!(
                        "### Session {} ({})\n\n",
                        prev.id, prev.created_at
                    ));
                    if let Some(summary) = &prev.summary {
                        output.push_str(&format!("{summary}\n\n"));
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

    pub fn search(&self, query: &str, limit: usize, session_id: Option<&str>) -> Result<String> {
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
        tracing::debug!(
            session_id = session_id,
            content_len = content.len(),
            "Saving observation"
        );
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

        // Auto-link: detect symbol names and file paths in content
        auto_link_observation(&conn, obs_id, content)?;

        crate::metrics::record_observation_saved();

        Ok(format!(
            "Observation saved (id: {obs_id}).\n**Headline:** {headline}"
        ))
    }
}

fn generate_headline(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or(content);
    if first_line.len() <= 80 {
        first_line.to_string()
    } else {
        // Use char_indices to find a safe UTF-8 boundary
        let end = first_line
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= 77)
            .last()
            .unwrap_or(0);
        format!("{}...", &first_line[..end])
    }
}

fn generate_summary(content: &str) -> String {
    if content.len() <= 200 {
        content.to_string()
    } else {
        let end = content
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= 197)
            .last()
            .unwrap_or(0);
        format!("{}...", &content[..end])
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

/// Common English words to filter out from PascalCase symbol detection.
const COMMON_WORDS: &[&str] = &[
    "The",
    "This",
    "That",
    "These",
    "Those",
    "When",
    "Where",
    "Which",
    "What",
    "With",
    "Without",
    "About",
    "After",
    "Before",
    "Between",
    "During",
    "From",
    "Into",
    "Through",
    "Under",
    "Until",
    "Upon",
    "Also",
    "Both",
    "Each",
    "Every",
    "Some",
    "None",
    "Many",
    "Much",
    "Most",
    "Other",
    "Such",
    "Than",
    "Then",
    "Only",
    "Just",
    "Even",
    "Still",
    "Already",
    "Always",
    "Never",
    "Often",
    "Sometimes",
    "However",
    "Therefore",
    "Because",
    "Although",
    "Whether",
    "While",
    "Since",
    "Though",
    "Unless",
    "Once",
    "Here",
    "There",
    "Note",
    "Todo",
    "Fixme",
    "Hack",
    "Warning",
    "Error",
];

/// Auto-detect symbol names and file paths in observation content and link them.
fn auto_link_observation(conn: &rusqlite::Connection, obs_id: i64, content: &str) -> Result<()> {
    use std::collections::HashSet;

    let mut linked_nodes = HashSet::new();
    let mut linked_files = HashSet::new();

    // Extract candidate symbol names
    // 1. PascalCase identifiers (3+ chars, starts with uppercase)
    // 2. Identifiers containing _ or :: (3+ chars)
    let candidates = extract_symbol_candidates(content);
    tracing::debug!(
        candidate_count = candidates.len(),
        "Auto-linking observation symbols"
    );

    // Auto-link symbols (up to 10)
    for candidate in candidates.iter().take(30) {
        if linked_nodes.len() >= 10 {
            break;
        }

        // Prefer exported/pub nodes
        let node_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM nodes WHERE name = ?1
                 AND (is_export = 1 OR visibility = 'pub')
                 LIMIT 1",
                rusqlite::params![candidate],
                |row| row.get(0),
            )
            .ok()
            .or_else(|| {
                conn.query_row(
                    "SELECT id FROM nodes WHERE name = ?1 LIMIT 1",
                    rusqlite::params![candidate],
                    |row| row.get(0),
                )
                .ok()
            });

        if let Some(nid) = node_id {
            if linked_nodes.insert(nid) {
                if let Err(e) = observation::link_observation_symbol(conn, obs_id, nid) {
                    tracing::debug!(error = %e, node_id = nid, "Failed to link observation symbol");
                }
            }
        }
    }

    // Scan for file paths (containing / with code extension)
    for word in content.split_whitespace() {
        if linked_files.len() >= 10 {
            break;
        }

        let cleaned = word.trim_matches(|c: char| {
            c == '`'
                || c == '\''
                || c == '"'
                || c == ','
                || c == '.'
                || c == ':'
                || c == ')'
                || c == '('
        });
        if cleaned.contains('/') {
            let path = std::path::Path::new(cleaned);
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if crate::types::Language::from_extension(ext).is_some() {
                    if let Ok(Some(file)) = crate::db::queries::get_file_by_path(conn, cleaned) {
                        if linked_files.insert(file.id) {
                            let _ = observation::link_observation_file(
                                conn,
                                obs_id,
                                file.id,
                                &file.content_hash,
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn extract_symbol_candidates(content: &str) -> Vec<String> {
    use std::collections::HashSet;

    let common: HashSet<&str> = COMMON_WORDS.iter().copied().collect();
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for word in content.split(|c: char| {
        c.is_whitespace()
            || c == ','
            || c == '('
            || c == ')'
            || c == '['
            || c == ']'
            || c == '{'
            || c == '}'
    }) {
        let cleaned = word.trim_matches(|c: char| {
            c == '`' || c == '\'' || c == '"' || c == '.' || c == ':' || c == ';'
        });

        if cleaned.len() < 3 {
            continue;
        }

        // PascalCase: starts with uppercase, has lowercase chars
        let is_pascal = cleaned.starts_with(|c: char| c.is_uppercase())
            && cleaned.chars().any(|c| c.is_lowercase())
            && cleaned.chars().all(|c| c.is_alphanumeric() || c == '_');

        // snake_case or qualified: contains _ or ::
        let is_snake_or_qualified = (cleaned.contains('_') || cleaned.contains("::"))
            && cleaned
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == ':');

        if (is_pascal || is_snake_or_qualified)
            && !common.contains(cleaned)
            && seen.insert(cleaned.to_string())
        {
            candidates.push(cleaned.to_string());
        }
    }

    candidates
}
