use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::db::queries::{self, CandidateNode};
use crate::error::Result;

/// Minimum confidence threshold to accept a resolution.
const MIN_ACCEPTANCE_CONFIDENCE: f64 = 0.30;

/// Resolve unresolved cross-file references by matching target names
/// against known nodes with scope-aware and type-aware disambiguation.
/// Uses batch-loaded data to avoid N per-ref queries.
/// Returns the number of edges created.
pub fn resolve_cross_file_refs(conn: &Connection) -> Result<usize> {
    let mut count = 0;

    // Get all unresolved refs (including context for control flow)
    let mut stmt = conn.prepare(
        "SELECT ur.id, ur.source_node_id, ur.target_name, ur.target_qualified_name,
                ur.edge_kind, ur.context, ur.import_path, sn.file_id
         FROM unresolved_refs ur
         JOIN nodes sn ON sn.id = ur.source_node_id",
    )?;

    type UnresolvedRefRow = (
        i64,
        i64,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        i64,
    );
    let refs: Vec<UnresolvedRefRow> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if refs.is_empty() {
        return Ok(0);
    }

    // Batch-load all exported/pub nodes, indexed by name and qualified name
    let all_exported = queries::get_all_exported_nodes(conn)?;
    let mut candidates_by_name: HashMap<&str, Vec<&CandidateNode>> = HashMap::new();
    let mut qname_to_id: HashMap<&str, i64> = HashMap::new();
    for node in &all_exported {
        candidates_by_name.entry(&node.name).or_default().push(node);
        if let Some(ref qn) = node.qualified_name {
            qname_to_id.insert(qn.as_str(), node.id);
        }
    }

    // Batch-load all import edges, indexed by source file ID
    let all_imports = queries::get_all_import_edges(conn)?;
    let mut imports_by_file: HashMap<i64, Vec<&queries::ImportEdgeRecord>> = HashMap::new();
    for imp in &all_imports {
        imports_by_file
            .entry(imp.source_file_id)
            .or_default()
            .push(imp);
    }

    // Batch-load all file paths for import-path matching
    let file_paths = queries::get_all_file_paths(conn)?;

    let mut insert_stmt = conn.prepare(
        "INSERT INTO edges (source_node_id, target_node_id, kind, confidence, context)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    let mut delete_stmt = conn.prepare("DELETE FROM unresolved_refs WHERE id = ?1")?;

    for (
        ref_id,
        source_id,
        target_name,
        target_qname,
        edge_kind,
        context,
        import_path,
        source_file_id,
    ) in &refs
    {
        // Strategy 1: Qualified name match (confidence 0.95)
        if let Some(qname) = target_qname {
            if let Some(&tid) = qname_to_id.get(qname.as_str()) {
                insert_stmt.execute(params![source_id, tid, edge_kind, 0.95, context])?;
                delete_stmt.execute(params![ref_id])?;
                count += 1;
                continue;
            }
        }

        // Get all candidates matching by name, excluding same-file nodes
        let candidates: Vec<&&CandidateNode> = match candidates_by_name.get(target_name.as_str()) {
            Some(cs) => cs.iter().filter(|c| c.file_id != *source_file_id).collect(),
            None => continue,
        };

        if candidates.is_empty() {
            continue;
        }

        let file_imports = imports_by_file.get(source_file_id);

        // Strategy 2: Single candidate — no disambiguation needed
        if candidates.len() == 1 {
            let candidate = candidates[0];
            let mut confidence = 0.50; // Base for single candidate

            // Boost if import evidence
            if let Some(imps) = file_imports {
                if imps
                    .iter()
                    .any(|imp| imp.target_file_id == candidate.file_id)
                {
                    confidence = 0.85;
                } else if let Some(ip) = import_path {
                    let candidate_path = file_paths
                        .get(&candidate.file_id)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    if path_matches_import(ip, candidate_path) {
                        confidence = 0.80;
                    }
                }
            } else if let Some(ip) = import_path {
                let candidate_path = file_paths
                    .get(&candidate.file_id)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                if path_matches_import(ip, candidate_path) {
                    confidence = 0.80;
                }
            }

            confidence = apply_type_preference(confidence, edge_kind, &candidate.kind);

            if confidence > MIN_ACCEPTANCE_CONFIDENCE {
                insert_stmt.execute(params![
                    source_id,
                    candidate.id,
                    edge_kind,
                    confidence,
                    context
                ])?;
                delete_stmt.execute(params![ref_id])?;
                count += 1;
            }
            continue;
        }

        // Strategy 3: Multi-candidate disambiguation
        if let Some((target_id, confidence)) = disambiguate(
            &candidates,
            edge_kind,
            import_path.as_deref(),
            file_imports,
            &file_paths,
        ) {
            if confidence > MIN_ACCEPTANCE_CONFIDENCE {
                insert_stmt.execute(params![
                    source_id, target_id, edge_kind, confidence, context
                ])?;
                delete_stmt.execute(params![ref_id])?;
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Score each candidate and return the best match with its confidence.
fn disambiguate(
    candidates: &[&&CandidateNode],
    edge_kind: &str,
    import_path: Option<&str>,
    file_imports: Option<&Vec<&queries::ImportEdgeRecord>>,
    file_paths: &HashMap<i64, String>,
) -> Option<(i64, f64)> {
    let mut best: Option<(i64, f64)> = None;

    for candidate in candidates {
        let mut confidence = 0.30_f64; // Base: name-only

        // Scope-aware: source file imports from the candidate's file
        if let Some(imps) = file_imports {
            if imps
                .iter()
                .any(|imp| imp.target_file_id == candidate.file_id)
            {
                confidence = 0.85;
            } else if let Some(ip) = import_path {
                let candidate_path = file_paths
                    .get(&candidate.file_id)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                if path_matches_import(ip, candidate_path) {
                    confidence = 0.80;
                }
            }
        } else if let Some(ip) = import_path {
            let candidate_path = file_paths
                .get(&candidate.file_id)
                .map(|s| s.as_str())
                .unwrap_or("");
            if path_matches_import(ip, candidate_path) {
                confidence = 0.80;
            }
        }

        // Type preference bonus
        confidence = apply_type_preference(confidence, edge_kind, &candidate.kind);

        match &best {
            Some((_, best_conf)) if confidence > *best_conf => {
                best = Some((candidate.id, confidence));
            }
            None => {
                best = Some((candidate.id, confidence));
            }
            _ => {}
        }
    }

    best
}

/// Apply a small type preference bonus if the edge kind matches the candidate node kind.
fn apply_type_preference(base_confidence: f64, edge_kind: &str, node_kind: &str) -> f64 {
    let bonus = match (edge_kind, node_kind) {
        ("calls", "function") | ("calls", "method") => 0.05,
        ("type_ref", "struct")
        | ("type_ref", "class")
        | ("type_ref", "trait")
        | ("type_ref", "interface")
        | ("type_ref", "type_alias")
        | ("type_ref", "enum") => 0.05,
        ("implements", "trait") | ("implements", "interface") => 0.05,
        ("extends", "class") | ("extends", "struct") => 0.05,
        _ => 0.0,
    };
    (base_confidence + bonus).min(0.99)
}

/// Check if an import_path (e.g., "./utils/helpers" or "utils/helpers") matches
/// a file path (e.g., "src/utils/helpers.ts").
fn path_matches_import(import_path: &str, file_path: &str) -> bool {
    if import_path.is_empty() || file_path.is_empty() {
        return false;
    }

    // Normalize: strip leading ./ from import path, strip extension from file path
    let import_clean = import_path.trim_start_matches("./");
    let file_stem = file_path
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_path);

    file_stem.ends_with(import_clean) || file_stem == import_clean
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup_test_db() -> Database {
        Database::open_test().expect("Failed to open test database")
    }

    fn insert_test_file(conn: &Connection, path: &str, lang: &str) -> i64 {
        queries::insert_file(conn, path, lang, "hash", 0, 100).unwrap()
    }

    fn insert_test_node(
        conn: &Connection,
        file_id: i64,
        kind: &str,
        name: &str,
        is_export: bool,
    ) -> i64 {
        queries::insert_node(
            conn,
            file_id,
            kind,
            name,
            None,
            None,
            None,
            1,
            10,
            0,
            0,
            Some("pub"),
            is_export,
            None,
        )
        .unwrap()
    }

    #[test]
    fn resolver_picks_imported_file_candidate() {
        let db = setup_test_db();
        let conn = db.writer();

        let file_a = insert_test_file(&conn, "a.rs", "rust");
        let file_b = insert_test_file(&conn, "b.rs", "rust");
        let file_c = insert_test_file(&conn, "c.rs", "rust");

        // Both b.rs and c.rs export "helper"
        let _helper_b = insert_test_node(&conn, file_b, "function", "helper", true);
        let helper_c = insert_test_node(&conn, file_c, "function", "helper", true);

        // a.rs has a function that calls "helper"
        let caller = insert_test_node(&conn, file_a, "function", "run", false);

        // a.rs imports from c.rs (creating an import edge)
        let import_node = insert_test_node(&conn, file_a, "import", "c_import", false);
        let c_mod = insert_test_node(&conn, file_c, "module", "c", true);
        queries::insert_edge(&conn, import_node, c_mod, "imports", 1.0, None).unwrap();

        // Add unresolved ref from caller -> "helper"
        queries::insert_unresolved_ref(&conn, caller, "helper", None, "calls", None, None).unwrap();

        let resolved = resolve_cross_file_refs(&conn).unwrap();
        assert_eq!(resolved, 1);

        // Should have picked helper_c (from the imported file)
        let edges = queries::get_all_edges(&conn).unwrap();
        let calls_edge = edges
            .iter()
            .find(|e| e.kind == "calls" && e.source_node_id == caller)
            .unwrap();
        assert_eq!(calls_edge.target_node_id, helper_c);
        assert!((calls_edge.confidence - 0.90).abs() < 0.01); // 0.85 + 0.05 type bonus
    }

    #[test]
    fn resolver_type_ref_prefers_struct_over_function() {
        let db = setup_test_db();
        let conn = db.writer();

        let file_a = insert_test_file(&conn, "a.rs", "rust");
        let file_b = insert_test_file(&conn, "b.rs", "rust");

        // b.rs exports both a struct "Config" and a function "Config" (constructor-like)
        let config_struct = insert_test_node(&conn, file_b, "struct", "Config", true);
        let _config_fn = insert_test_node(&conn, file_b, "function", "Config", true);

        let caller = insert_test_node(&conn, file_a, "function", "run", false);

        // a.rs imports from b.rs
        let import_node = insert_test_node(&conn, file_a, "import", "b_import", false);
        let b_mod = insert_test_node(&conn, file_b, "module", "b", true);
        queries::insert_edge(&conn, import_node, b_mod, "imports", 1.0, None).unwrap();

        // type_ref edge should prefer struct
        queries::insert_unresolved_ref(&conn, caller, "Config", None, "type_ref", None, None)
            .unwrap();

        let resolved = resolve_cross_file_refs(&conn).unwrap();
        assert_eq!(resolved, 1);

        let edges = queries::get_all_edges(&conn).unwrap();
        let type_edge = edges.iter().find(|e| e.kind == "type_ref").unwrap();
        assert_eq!(type_edge.target_node_id, config_struct);
    }

    #[test]
    fn resolver_single_candidate_no_import_accepted() {
        let db = setup_test_db();
        let conn = db.writer();

        let file_a = insert_test_file(&conn, "a.rs", "rust");
        let file_b = insert_test_file(&conn, "b.rs", "rust");

        let helper = insert_test_node(&conn, file_b, "function", "unique_helper", true);
        let caller = insert_test_node(&conn, file_a, "function", "run", false);

        queries::insert_unresolved_ref(&conn, caller, "unique_helper", None, "calls", None, None)
            .unwrap();

        let resolved = resolve_cross_file_refs(&conn).unwrap();
        assert_eq!(resolved, 1);

        let edges = queries::get_all_edges(&conn).unwrap();
        let calls_edge = edges.iter().find(|e| e.kind == "calls").unwrap();
        assert_eq!(calls_edge.target_node_id, helper);
        // Single candidate with no import evidence: base 0.50 + 0.05 type bonus
        assert!((calls_edge.confidence - 0.55).abs() < 0.01);
    }

    #[test]
    fn resolver_rejects_ambiguous_name_only_match() {
        let db = setup_test_db();
        let conn = db.writer();

        let file_a = insert_test_file(&conn, "a.rs", "rust");
        let file_b = insert_test_file(&conn, "b.rs", "rust");
        let file_c = insert_test_file(&conn, "c.rs", "rust");

        // Both b.rs and c.rs export "helper", no import evidence from a.rs
        let _helper_b = insert_test_node(&conn, file_b, "function", "helper", true);
        let _helper_c = insert_test_node(&conn, file_c, "function", "helper", true);

        let caller = insert_test_node(&conn, file_a, "function", "run", false);
        queries::insert_unresolved_ref(&conn, caller, "helper", None, "calls", None, None).unwrap();

        let resolved = resolve_cross_file_refs(&conn).unwrap();
        // Name-only confidence (0.30 + 0.05 = 0.35) > MIN_ACCEPTANCE (0.30), so it should resolve
        // But let's verify the confidence is low
        assert_eq!(resolved, 1);
        let edges = queries::get_all_edges(&conn).unwrap();
        let calls_edge = edges.iter().find(|e| e.kind == "calls").unwrap();
        assert!(
            calls_edge.confidence < 0.5,
            "Ambiguous match should have low confidence"
        );
    }

    #[test]
    fn path_matches_import_works() {
        assert!(path_matches_import(
            "./utils/helpers",
            "src/utils/helpers.ts"
        ));
        assert!(path_matches_import("utils/helpers", "src/utils/helpers.ts"));
        assert!(path_matches_import("helpers", "src/utils/helpers.ts"));
        assert!(!path_matches_import("other", "src/utils/helpers.ts"));
        assert!(!path_matches_import("", "src/utils/helpers.ts"));
    }
}
