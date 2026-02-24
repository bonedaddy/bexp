use tree_sitter::{Node, Tree};

use crate::indexer::extractor::*;
use crate::types::*;

pub struct PythonExtractor;

impl LanguageExtractor for PythonExtractor {
    fn language(&self) -> Language {
        Language::Python
    }

    fn extract(&self, tree: &Tree, source: &str, file_path: &str) -> ExtractedFile {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut unresolved_refs = Vec::new();

        let root = tree.root_node();
        extract_from_node(root, source, file_path, &mut nodes, &mut edges, &mut unresolved_refs, None);

        ExtractedFile {
            language: Language::Python,
            content_hash: String::new(),
            mtime_ns: 0,
            size_bytes: 0,
            nodes,
            edges,
            unresolved_refs,
        }
    }
}

fn get_node_text<'a>(node: Node, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

fn extract_from_node(
    node: Node,
    source: &str,
    file_path: &str,
    nodes: &mut Vec<ExtractedNode>,
    edges: &mut Vec<ExtractedEdge>,
    unresolved_refs: &mut Vec<UnresolvedRef>,
    parent_idx: Option<usize>,
) {
    match node.kind() {
        "function_definition" => {
            if let Some(extracted) = extract_function(node, source, file_path) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    edges.push(ExtractedEdge {
                        source_idx: parent,
                        target_idx: idx,
                        kind: EdgeKind::Contains,
                        confidence: 1.0,
                    });
                }
                extract_calls(node, source, idx, unresolved_refs);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
            }
        }
        "class_definition" => {
            if let Some(extracted) = extract_class(node, source, file_path) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    edges.push(ExtractedEdge {
                        source_idx: parent,
                        target_idx: idx,
                        kind: EdgeKind::Contains,
                        confidence: 1.0,
                    });
                }
                // Check for base classes
                if let Some(args) = find_child_by_kind(node, "argument_list") {
                    for i in 0..args.child_count() {
                        if let Some(arg) = args.child(i as u32) {
                            if arg.kind() == "identifier" {
                                let name = get_node_text(arg, source).to_string();
                                unresolved_refs.push(UnresolvedRef {
                                    source_idx: idx,
                                    target_name: name,
                                    target_qualified_name: None,
                                    edge_kind: EdgeKind::Extends,
                                    import_path: None,
                                });
                            }
                        }
                    }
                }
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
            }
        }
        "import_statement" | "import_from_statement" => {
            extract_import(node, source, file_path, nodes, unresolved_refs);
        }
        "assignment" | "expression_statement" => {
            // Module-level assignments can define constants/variables
            if parent_idx.is_none() {
                if let Some(extracted) = extract_assignment(node, source, file_path) {
                    let _idx = nodes.len();
                    nodes.push(extracted);
                }
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, parent_idx);
        }
    }
}

fn extract_function(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")?;
    let name = get_node_text(name_node, source).to_string();

    let is_method = node
        .parent()
        .map(|p| p.kind() == "block")
        .unwrap_or(false)
        && node
            .parent()
            .and_then(|p| p.parent())
            .map(|gp| gp.kind() == "class_definition")
            .unwrap_or(false);

    let kind = if is_method {
        NodeKind::Method
    } else {
        NodeKind::Function
    };

    let params = find_child_by_kind(node, "parameters")
        .map(|p| get_node_text(p, source).to_string())
        .unwrap_or_else(|| "()".to_string());

    let return_type = find_child_by_kind(node, "type")
        .map(|t| format!(" -> {}", get_node_text(t, source)));

    let signature = format!("def {}{}{}", name, params, return_type.unwrap_or_default());
    let qualified_name = format!("{}::{}", file_path, name);

    // Extract docstring
    let docstring = find_child_by_kind(node, "block")
        .and_then(|block| block.child(0))
        .and_then(|first| {
            if first.kind() == "expression_statement" {
                first.child(0)
            } else {
                None
            }
        })
        .and_then(|expr| {
            if expr.kind() == "string" {
                Some(get_node_text(expr, source).trim_matches('"').trim_matches('\'').to_string())
            } else {
                None
            }
        });

    let visibility = if name.starts_with("__") && !name.ends_with("__") {
        Some("private".to_string())
    } else if name.starts_with('_') {
        Some("protected".to_string())
    } else {
        Some("public".to_string())
    };

    Some(ExtractedNode {
        kind,
        name,
        qualified_name: Some(qualified_name),
        signature: Some(signature),
        docstring,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility,
        is_export: false,
    })
}

fn extract_class(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);

    let docstring = find_child_by_kind(node, "block")
        .and_then(|block| block.child(0))
        .and_then(|first| {
            if first.kind() == "expression_statement" {
                first.child(0)
            } else {
                None
            }
        })
        .and_then(|expr| {
            if expr.kind() == "string" {
                Some(get_node_text(expr, source).trim_matches('"').trim_matches('\'').to_string())
            } else {
                None
            }
        });

    Some(ExtractedNode {
        kind: NodeKind::Class,
        name,
        qualified_name: Some(qualified_name),
        signature: None,
        docstring,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: Some("public".to_string()),
        is_export: false,
    })
}

fn extract_import(
    node: Node,
    source: &str,
    _file_path: &str,
    nodes: &mut Vec<ExtractedNode>,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    let text = get_node_text(node, source).to_string();

    // Extract module name
    let module_name = find_child_by_kind(node, "dotted_name")
        .map(|n| get_node_text(n, source).to_string());

    let idx = nodes.len();
    nodes.push(ExtractedNode {
        kind: NodeKind::Import,
        name: text.lines().next().unwrap_or(&text).to_string(),
        qualified_name: None,
        signature: None,
        docstring: None,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export: false,
    });

    // Extract imported names
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "aliased_import" || child.kind() == "dotted_name" {
                if let Some(name_node) = child.child(0) {
                    if name_node.kind() == "identifier" {
                        let name = get_node_text(name_node, source).to_string();
                        unresolved_refs.push(UnresolvedRef {
                            source_idx: idx,
                            target_name: name,
                            target_qualified_name: None,
                            edge_kind: EdgeKind::Imports,
                            import_path: module_name.clone(),
                        });
                    }
                }
            } else if child.kind() == "identifier" {
                let name = get_node_text(child, source).to_string();
                unresolved_refs.push(UnresolvedRef {
                    source_idx: idx,
                    target_name: name,
                    target_qualified_name: None,
                    edge_kind: EdgeKind::Imports,
                    import_path: module_name.clone(),
                });
            }
        }
    }
}

fn extract_assignment(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let actual = if node.kind() == "expression_statement" {
        node.child(0)?
    } else {
        node
    };
    if actual.kind() != "assignment" {
        return None;
    }

    let left = actual.child(0)?;
    if left.kind() != "identifier" {
        return None;
    }

    let name = get_node_text(left, source).to_string();
    // Only capture UPPER_CASE assignments as constants
    if !name.chars().all(|c| c.is_uppercase() || c == '_') {
        return None;
    }

    let qualified_name = format!("{}::{}", file_path, name);

    Some(ExtractedNode {
        kind: NodeKind::Constant,
        name,
        qualified_name: Some(qualified_name),
        signature: Some(get_node_text(actual, source).lines().next().unwrap_or("").to_string()),
        docstring: None,
        line_start: actual.start_position().row + 1,
        line_end: actual.end_position().row + 1,
        col_start: actual.start_position().column,
        col_end: actual.end_position().column,
        visibility: Some("public".to_string()),
        is_export: false,
    })
}

fn extract_calls(
    node: Node,
    source: &str,
    parent_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    if node.kind() == "call" {
        if let Some(func) = node.child(0) {
            let name = get_node_text(func, source).to_string();
            unresolved_refs.push(UnresolvedRef {
                source_idx: parent_idx,
                target_name: name,
                target_qualified_name: None,
                edge_kind: EdgeKind::Calls,
                import_path: None,
            });
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "function_definition" || child.kind() == "lambda" {
                continue;
            }
            extract_calls(child, source, parent_idx, unresolved_refs);
        }
    }
}
