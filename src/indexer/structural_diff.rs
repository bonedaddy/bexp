use std::collections::HashMap;

use crate::types::NodeSummary;

#[derive(Debug, Clone)]
pub struct StructuralChange {
    pub file_path: String,
    pub added_nodes: Vec<NodeDiff>,
    pub removed_nodes: Vec<NodeDiff>,
    pub modified_nodes: Vec<NodeModification>,
    pub renamed_nodes: Vec<NodeRename>,
}

#[derive(Debug, Clone)]
pub struct NodeDiff {
    pub kind: String,
    pub name: String,
    pub qualified_name: Option<String>,
    pub line_start: i64,
    pub line_end: i64,
}

#[derive(Debug, Clone)]
pub struct NodeModification {
    pub kind: String,
    pub name: String,
    pub old_signature: Option<String>,
    pub new_signature: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NodeRename {
    pub kind: String,
    pub old_name: String,
    pub new_name: String,
    pub line_start: i64,
}

/// Compute the structural diff between old and new node summaries.
pub fn compute_structural_diff(
    file_path: &str,
    old_nodes: &[NodeSummary],
    new_nodes: &[NodeSummary],
) -> StructuralChange {
    // Index by (kind, name) for matching
    let mut old_map: HashMap<(&str, &str), Vec<&NodeSummary>> = HashMap::new();
    for node in old_nodes {
        old_map
            .entry((node.kind.as_str(), node.name.as_str()))
            .or_default()
            .push(node);
    }

    let mut new_map: HashMap<(&str, &str), Vec<&NodeSummary>> = HashMap::new();
    for node in new_nodes {
        new_map
            .entry((node.kind.as_str(), node.name.as_str()))
            .or_default()
            .push(node);
    }

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();
    let mut renamed = Vec::new();

    // Find matched, modified, and added
    let mut matched_old_keys: std::collections::HashSet<(&str, &str)> =
        std::collections::HashSet::new();

    for (&key, new_entries) in &new_map {
        if let Some(old_entries) = old_map.get(&key) {
            matched_old_keys.insert(key);
            // Check for signature changes
            let old_entry = old_entries[0];
            let new_entry = new_entries[0];
            if old_entry.signature != new_entry.signature {
                modified.push(NodeModification {
                    kind: key.0.to_string(),
                    name: key.1.to_string(),
                    old_signature: old_entry.signature.clone(),
                    new_signature: new_entry.signature.clone(),
                });
            }
        } else {
            // New node — check if it's a rename (same kind, similar line position)
            let new_entry = new_entries[0];
            let mut is_rename = false;

            for (&old_key, old_entries) in &old_map {
                if old_key.0 == key.0
                    && old_key.1 != key.1
                    && !matched_old_keys.contains(&old_key)
                    && !new_map.contains_key(&old_key)
                {
                    let old_entry = old_entries[0];
                    // Within 5 lines proximity = likely rename
                    if (old_entry.line_start - new_entry.line_start).abs() <= 5 {
                        renamed.push(NodeRename {
                            kind: key.0.to_string(),
                            old_name: old_key.1.to_string(),
                            new_name: key.1.to_string(),
                            line_start: new_entry.line_start,
                        });
                        matched_old_keys.insert(old_key);
                        is_rename = true;
                        break;
                    }
                }
            }

            if !is_rename {
                added.push(NodeDiff {
                    kind: key.0.to_string(),
                    name: key.1.to_string(),
                    qualified_name: None,
                    line_start: new_entry.line_start,
                    line_end: new_entry.line_end,
                });
            }
        }
    }

    // Find removed (old keys not in new_map and not matched as renames)
    for (&key, entries) in &old_map {
        if !new_map.contains_key(&key) && !matched_old_keys.contains(&key) {
            let entry = entries[0];
            removed.push(NodeDiff {
                kind: key.0.to_string(),
                name: key.1.to_string(),
                qualified_name: None,
                line_start: entry.line_start,
                line_end: entry.line_end,
            });
        }
    }

    StructuralChange {
        file_path: file_path.to_string(),
        added_nodes: added,
        removed_nodes: removed,
        modified_nodes: modified,
        renamed_nodes: renamed,
    }
}

impl StructuralChange {
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.modified_nodes.is_empty()
            && self.renamed_nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_changes_produces_empty_diff() {
        let nodes = vec![NodeSummary {
            kind: "function".to_string(),
            name: "foo".to_string(),
            signature: Some("fn foo()".to_string()),
            line_start: 1,
            line_end: 5,
        }];
        let diff = compute_structural_diff("test.rs", &nodes, &nodes);
        assert!(diff.is_empty());
    }

    #[test]
    fn detects_added_node() {
        let old = vec![NodeSummary {
            kind: "function".to_string(),
            name: "foo".to_string(),
            signature: Some("fn foo()".to_string()),
            line_start: 1,
            line_end: 5,
        }];
        let new = vec![
            NodeSummary {
                kind: "function".to_string(),
                name: "foo".to_string(),
                signature: Some("fn foo()".to_string()),
                line_start: 1,
                line_end: 5,
            },
            NodeSummary {
                kind: "function".to_string(),
                name: "bar".to_string(),
                signature: Some("fn bar()".to_string()),
                line_start: 7,
                line_end: 10,
            },
        ];
        let diff = compute_structural_diff("test.rs", &old, &new);
        assert_eq!(diff.added_nodes.len(), 1);
        assert_eq!(diff.added_nodes[0].name, "bar");
    }

    #[test]
    fn detects_removed_node() {
        let old = vec![
            NodeSummary {
                kind: "function".to_string(),
                name: "foo".to_string(),
                signature: Some("fn foo()".to_string()),
                line_start: 1,
                line_end: 5,
            },
            NodeSummary {
                kind: "function".to_string(),
                name: "bar".to_string(),
                signature: Some("fn bar()".to_string()),
                line_start: 7,
                line_end: 10,
            },
        ];
        let new = vec![NodeSummary {
            kind: "function".to_string(),
            name: "foo".to_string(),
            signature: Some("fn foo()".to_string()),
            line_start: 1,
            line_end: 5,
        }];
        let diff = compute_structural_diff("test.rs", &old, &new);
        assert_eq!(diff.removed_nodes.len(), 1);
        assert_eq!(diff.removed_nodes[0].name, "bar");
    }

    #[test]
    fn detects_modified_signature() {
        let old = vec![NodeSummary {
            kind: "function".to_string(),
            name: "foo".to_string(),
            signature: Some("fn foo()".to_string()),
            line_start: 1,
            line_end: 5,
        }];
        let new = vec![NodeSummary {
            kind: "function".to_string(),
            name: "foo".to_string(),
            signature: Some("fn foo(x: i32)".to_string()),
            line_start: 1,
            line_end: 5,
        }];
        let diff = compute_structural_diff("test.rs", &old, &new);
        assert_eq!(diff.modified_nodes.len(), 1);
        assert_eq!(
            diff.modified_nodes[0].old_signature,
            Some("fn foo()".to_string())
        );
        assert_eq!(
            diff.modified_nodes[0].new_signature,
            Some("fn foo(x: i32)".to_string())
        );
    }

    #[test]
    fn format_only_change_same_hash() {
        use crate::indexer::extractor::{compute_structure_hash, ExtractedEdge, ExtractedNode};
        use crate::types::NodeKind;

        let nodes = vec![ExtractedNode {
            kind: NodeKind::Function,
            name: "foo".to_string(),
            qualified_name: None,
            signature: Some("fn foo()".to_string()),
            docstring: None,
            line_start: 1,
            line_end: 5,
            col_start: 0,
            col_end: 0,
            visibility: None,
            is_export: false,
            metadata: None,
        }];
        let edges: Vec<ExtractedEdge> = vec![];

        let hash1 = compute_structure_hash(&nodes, &edges);
        let hash2 = compute_structure_hash(&nodes, &edges);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn signature_change_different_hash() {
        use crate::indexer::extractor::{compute_structure_hash, ExtractedEdge, ExtractedNode};
        use crate::types::NodeKind;

        let nodes1 = vec![ExtractedNode {
            kind: NodeKind::Function,
            name: "foo".to_string(),
            qualified_name: None,
            signature: Some("fn foo()".to_string()),
            docstring: None,
            line_start: 1,
            line_end: 5,
            col_start: 0,
            col_end: 0,
            visibility: None,
            is_export: false,
            metadata: None,
        }];
        let nodes2 = vec![ExtractedNode {
            kind: NodeKind::Function,
            name: "foo".to_string(),
            qualified_name: None,
            signature: Some("fn foo(x: i32)".to_string()),
            docstring: None,
            line_start: 1,
            line_end: 5,
            col_start: 0,
            col_end: 0,
            visibility: None,
            is_export: false,
            metadata: None,
        }];
        let edges: Vec<ExtractedEdge> = vec![];

        let hash1 = compute_structure_hash(&nodes1, &edges);
        let hash2 = compute_structure_hash(&nodes2, &edges);
        assert_ne!(hash1, hash2);
    }
}
