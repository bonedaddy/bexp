use tree_sitter::{Node, Tree};

use crate::indexer::extractor::*;
use crate::types::*;

pub struct TypeScriptExtractor;

impl LanguageExtractor for TypeScriptExtractor {
    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn extract(&self, tree: &Tree, source: &str, file_path: &str) -> ExtractedFile {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut unresolved_refs = Vec::new();

        let root = tree.root_node();
        extract_from_node(root, source, file_path, &mut nodes, &mut edges, &mut unresolved_refs, None);

        ExtractedFile {
            language: Language::TypeScript,
            content_hash: String::new(),
            mtime_ns: 0,
            size_bytes: 0,
            nodes,
            edges,
            unresolved_refs,
        }
    }
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
    let kind = node.kind();

    match kind {
        "function_declaration" | "method_definition" | "arrow_function" => {
            if let Some(extracted) = extract_function(node, source, file_path, kind) {
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
                // Extract call references within the body
                extract_calls(node, source, idx, unresolved_refs);
                // Recurse into children with this as parent
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
            }
        }
        "class_declaration" => {
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
                // Check for extends/implements
                extract_heritage(node, source, idx, unresolved_refs);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(extracted) = extract_interface(node, source, file_path) {
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
                extract_heritage(node, source, idx, unresolved_refs);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
            }
        }
        "type_alias_declaration" => {
            if let Some(extracted) = extract_type_alias(node, source, file_path) {
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
            }
        }
        "enum_declaration" => {
            if let Some(extracted) = extract_enum(node, source, file_path) {
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
            }
        }
        "import_statement" => {
            extract_import(node, source, file_path, nodes, unresolved_refs);
        }
        "export_statement" => {
            // Process the exported declaration
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, parent_idx);
                    // Mark last added node as export
                    if !nodes.is_empty() {
                        nodes.last_mut().unwrap().is_export = true;
                    }
                }
            }
            return;
        }
        "lexical_declaration" | "variable_declaration" => {
            extract_variable_declaration(node, source, file_path, nodes, parent_idx, edges);
        }
        _ => {}
    }

    // Default: recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, parent_idx);
        }
    }
}

fn get_node_text<'a>(node: Node, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

fn get_preceding_comment(node: Node, source: &str) -> Option<String> {
    if let Some(prev) = node.prev_sibling() {
        if prev.kind() == "comment" {
            let text = get_node_text(prev, source).trim().to_string();
            return Some(text);
        }
    }
    None
}

fn extract_function(node: Node, source: &str, file_path: &str, node_kind: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")
        .or_else(|| find_child_by_kind(node, "property_identifier"))?;
    let name = get_node_text(name_node, source).to_string();

    let kind = if node_kind == "method_definition" {
        NodeKind::Method
    } else {
        NodeKind::Function
    };

    // Build signature from parameters
    let params = find_child_by_kind(node, "formal_parameters")
        .map(|p| get_node_text(p, source).to_string())
        .unwrap_or_else(|| "()".to_string());

    let return_type = find_child_by_kind(node, "type_annotation")
        .map(|t| get_node_text(t, source).to_string());

    let signature = match return_type {
        Some(ret) => format!("{}{} {}", name, params, ret),
        None => format!("{}{}", name, params),
    };

    let qualified_name = format!("{}::{}", file_path, name);

    let is_export = node
        .parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false);

    let docstring = get_preceding_comment(node, source);

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
        visibility: None,
        is_export,
    })
}

fn extract_class(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")
        .or_else(|| find_child_by_kind(node, "identifier"))?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);

    let is_export = node
        .parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false);

    let docstring = get_preceding_comment(node, source);

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
        visibility: None,
        is_export,
    })
}

fn extract_interface(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")
        .or_else(|| find_child_by_kind(node, "identifier"))?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);

    let is_export = node
        .parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false);

    let docstring = get_preceding_comment(node, source);

    Some(ExtractedNode {
        kind: NodeKind::Interface,
        name,
        qualified_name: Some(qualified_name),
        signature: None,
        docstring,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export,
    })
}

fn extract_type_alias(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")
        .or_else(|| find_child_by_kind(node, "identifier"))?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let signature = Some(get_node_text(node, source).lines().next().unwrap_or("").to_string());

    let is_export = node
        .parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false);

    Some(ExtractedNode {
        kind: NodeKind::TypeAlias,
        name,
        qualified_name: Some(qualified_name),
        signature,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export,
    })
}

fn extract_enum(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);

    let is_export = node
        .parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false);

    Some(ExtractedNode {
        kind: NodeKind::Enum,
        name,
        qualified_name: Some(qualified_name),
        signature: None,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export,
    })
}

fn extract_import(
    node: Node,
    source: &str,
    _file_path: &str,
    nodes: &mut Vec<ExtractedNode>,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    let text = get_node_text(node, source);

    // Extract the module path
    let import_path = find_child_by_kind(node, "string")
        .map(|s| {
            let t = get_node_text(s, source);
            t.trim_matches(|c| c == '\'' || c == '"').to_string()
        });

    let idx = nodes.len();
    nodes.push(ExtractedNode {
        kind: NodeKind::Import,
        name: text.lines().next().unwrap_or(text).to_string(),
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

    // Extract imported names as unresolved refs
    if let Some(clause) = find_child_by_kind(node, "import_clause") {
        extract_import_names(clause, source, idx, &import_path, unresolved_refs);
    }
    if let Some(clause) = find_child_by_kind(node, "named_imports") {
        extract_import_names(clause, source, idx, &import_path, unresolved_refs);
    }
}

fn extract_import_names(
    node: Node,
    source: &str,
    source_idx: usize,
    import_path: &Option<String>,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            match child.kind() {
                "identifier" | "type_identifier" => {
                    let name = get_node_text(child, source).to_string();
                    unresolved_refs.push(UnresolvedRef {
                        source_idx,
                        target_name: name,
                        target_qualified_name: None,
                        edge_kind: EdgeKind::Imports,
                        import_path: import_path.clone(),
                    });
                }
                "import_specifier" => {
                    if let Some(name_node) = find_child_by_kind(child, "identifier") {
                        let name = get_node_text(name_node, source).to_string();
                        unresolved_refs.push(UnresolvedRef {
                            source_idx,
                            target_name: name,
                            target_qualified_name: None,
                            edge_kind: EdgeKind::Imports,
                            import_path: import_path.clone(),
                        });
                    }
                }
                "named_imports" => {
                    extract_import_names(child, source, source_idx, import_path, unresolved_refs);
                }
                _ => {}
            }
        }
    }
}

fn extract_heritage(
    node: Node,
    source: &str,
    class_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            match child.kind() {
                "class_heritage" | "extends_clause" => {
                    for j in 0..child.child_count() {
                        if let Some(name_node) = child.child(j) {
                            if name_node.kind() == "identifier" || name_node.kind() == "type_identifier" {
                                let name = get_node_text(name_node, source).to_string();
                                unresolved_refs.push(UnresolvedRef {
                                    source_idx: class_idx,
                                    target_name: name,
                                    target_qualified_name: None,
                                    edge_kind: EdgeKind::Extends,
                                    import_path: None,
                                });
                            }
                        }
                    }
                }
                "implements_clause" => {
                    for j in 0..child.child_count() {
                        if let Some(name_node) = child.child(j) {
                            if name_node.kind() == "identifier" || name_node.kind() == "type_identifier" {
                                let name = get_node_text(name_node, source).to_string();
                                unresolved_refs.push(UnresolvedRef {
                                    source_idx: class_idx,
                                    target_name: name,
                                    target_qualified_name: None,
                                    edge_kind: EdgeKind::Implements,
                                    import_path: None,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn extract_calls(
    node: Node,
    source: &str,
    parent_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    if node.kind() == "call_expression" {
        if let Some(func) = node.child(0) {
            let name = get_node_text(func, source).to_string();
            // Only track simple identifiers and member expressions
            let final_name = if name.contains('.') {
                name.clone()
            } else {
                name.clone()
            };
            unresolved_refs.push(UnresolvedRef {
                source_idx: parent_idx,
                target_name: final_name,
                target_qualified_name: None,
                edge_kind: EdgeKind::Calls,
                import_path: None,
            });
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            // Skip nested function bodies to avoid deep recursion of call extraction
            if child.kind() == "arrow_function" || child.kind() == "function" {
                continue;
            }
            extract_calls(child, source, parent_idx, unresolved_refs);
        }
    }
}

fn extract_variable_declaration(
    node: Node,
    source: &str,
    file_path: &str,
    nodes: &mut Vec<ExtractedNode>,
    parent_idx: Option<usize>,
    edges: &mut Vec<ExtractedEdge>,
) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "variable_declarator" {
                if let Some(name_node) = find_child_by_kind(child, "identifier") {
                    let name = get_node_text(name_node, source).to_string();
                    let qualified_name = format!("{}::{}", file_path, name);

                    // Check if it's a const (likely constant)
                    let text = get_node_text(node, source);
                    let kind = if text.starts_with("const") {
                        NodeKind::Constant
                    } else {
                        NodeKind::Variable
                    };

                    let is_export = node
                        .parent()
                        .map(|p| p.kind() == "export_statement")
                        .unwrap_or(false);

                    let idx = nodes.len();
                    nodes.push(ExtractedNode {
                        kind,
                        name,
                        qualified_name: Some(qualified_name),
                        signature: Some(text.lines().next().unwrap_or("").to_string()),
                        docstring: get_preceding_comment(node, source),
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        col_start: child.start_position().column,
                        col_end: child.end_position().column,
                        visibility: None,
                        is_export,
                    });

                    if let Some(parent) = parent_idx {
                        edges.push(ExtractedEdge {
                            source_idx: parent,
                            target_idx: idx,
                            kind: EdgeKind::Contains,
                            confidence: 1.0,
                        });
                    }
                }
            }
        }
    }
}
