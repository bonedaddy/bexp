use std::collections::HashSet;

use tree_sitter::{Node, Tree};

use crate::indexer::extractor::*;
use crate::types::*;

pub struct CExtractor;

impl LanguageExtractor for CExtractor {
    fn language(&self) -> Language {
        Language::C
    }

    fn extract(&self, tree: &Tree, source: &str, file_path: &str) -> ExtractedFile {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut unresolved_refs = Vec::new();

        let root = tree.root_node();
        extract_from_node(
            root,
            source,
            file_path,
            &mut nodes,
            &mut edges,
            &mut unresolved_refs,
            None,
        );

        // Detect env var usage: getenv("VAR")
        extract_env_vars_c(source, &mut nodes, &mut edges);

        ExtractedFile {
            language: Language::C,
            content_hash: String::new(),
            mtime_ns: 0,
            size_bytes: 0,
            nodes,
            edges,
            unresolved_refs,
            structure_hash: None,
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

fn find_child_by_field<'a>(node: Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

/// Recursively walk declarator nodes to find the underlying identifier.
/// C declarators can be nested: `pointer_declarator` -> `function_declarator` -> `identifier`.
fn find_identifier_in_declarator(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(get_node_text(node, source).to_string()),
        "pointer_declarator"
        | "function_declarator"
        | "array_declarator"
        | "parenthesized_declarator" => {
            // The declarator child is typically the first named child
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i as u32) {
                    if let Some(name) = find_identifier_in_declarator(child, source) {
                        return Some(name);
                    }
                }
            }
            None
        }
        "type_identifier" => Some(get_node_text(node, source).to_string()),
        _ => None,
    }
}

fn get_preceding_comment(node: Node, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(prev) = current {
        if prev.kind() == "comment" {
            comments.push(get_node_text(prev, source).to_string());
            current = prev.prev_sibling();
        } else {
            break;
        }
    }
    if comments.is_empty() {
        None
    } else {
        comments.reverse();
        Some(comments.join("\n"))
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
                        context: None,
                    });
                }
                extract_calls(node, source, idx, unresolved_refs);
                return;
            }
        }
        "struct_specifier" => {
            if let Some(extracted) = extract_struct(node, source, file_path) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    edges.push(ExtractedEdge {
                        source_idx: parent,
                        target_idx: idx,
                        kind: EdgeKind::Contains,
                        confidence: 1.0,
                        context: None,
                    });
                }
            }
        }
        "enum_specifier" => {
            if let Some(extracted) = extract_enum(node, source, file_path) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    edges.push(ExtractedEdge {
                        source_idx: parent,
                        target_idx: idx,
                        kind: EdgeKind::Contains,
                        confidence: 1.0,
                        context: None,
                    });
                }
            }
        }
        "type_definition" => {
            if let Some(extracted) = extract_typedef(node, source, file_path) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    edges.push(ExtractedEdge {
                        source_idx: parent,
                        target_idx: idx,
                        kind: EdgeKind::Contains,
                        confidence: 1.0,
                        context: None,
                    });
                }
            }
        }
        "preproc_include" => {
            extract_include(node, source, file_path, nodes, unresolved_refs);
        }
        "declaration" => {
            // Only extract top-level declarations (parent is translation_unit)
            if node
                .parent()
                .is_some_and(|p| p.kind() == "translation_unit")
            {
                if let Some(extracted) = extract_declaration(node, source, file_path) {
                    let idx = nodes.len();
                    nodes.push(extracted);
                    if let Some(parent) = parent_idx {
                        edges.push(ExtractedEdge {
                            source_idx: parent,
                            target_idx: idx,
                            kind: EdgeKind::Contains,
                            confidence: 1.0,
                            context: None,
                        });
                    }
                }
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            extract_from_node(
                child,
                source,
                file_path,
                nodes,
                edges,
                unresolved_refs,
                parent_idx,
            );
        }
    }
}

fn extract_function(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let declarator = find_child_by_field(node, "declarator")?;
    let name = find_identifier_in_declarator(declarator, source)?;

    let sig_end = find_child_by_kind(node, "compound_statement")
        .map(|b| b.start_byte())
        .unwrap_or(node.end_byte());
    let signature = source[node.start_byte()..sig_end].trim().to_string();
    let qualified_name = format!("{file_path}::{name}");

    Some(ExtractedNode {
        kind: NodeKind::Function,
        name,
        qualified_name: Some(qualified_name),
        signature: Some(signature),
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export: false,
        metadata: None,
    })
}

fn extract_struct(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_field(node, "name")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{file_path}::{name}");

    Some(ExtractedNode {
        kind: NodeKind::Struct,
        name,
        qualified_name: Some(qualified_name),
        signature: None,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export: false,
        metadata: None,
    })
}

fn extract_enum(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_field(node, "name")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{file_path}::{name}");

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
        is_export: false,
        metadata: None,
    })
}

fn extract_typedef(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let declarator = find_child_by_field(node, "declarator")?;
    let name = find_identifier_in_declarator(declarator, source)?;
    let qualified_name = format!("{file_path}::{name}");
    let signature = Some(
        get_node_text(node, source)
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
    );

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
        is_export: false,
        metadata: None,
    })
}

fn extract_include(
    node: Node,
    source: &str,
    _file_path: &str,
    nodes: &mut Vec<ExtractedNode>,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    let path_node = find_child_by_field(node, "path");
    let path_text = path_node
        .map(|n| get_node_text(n, source).to_string())
        .unwrap_or_else(|| get_node_text(node, source).to_string());

    let idx = nodes.len();
    nodes.push(ExtractedNode {
        kind: NodeKind::Import,
        name: path_text.clone(),
        qualified_name: None,
        signature: None,
        docstring: None,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export: false,
        metadata: None,
    });

    unresolved_refs.push(UnresolvedRef {
        source_idx: idx,
        target_name: path_text,
        target_qualified_name: None,
        edge_kind: EdgeKind::Imports,
        import_path: None,
        context: None,
    });
}

fn extract_declaration(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let declarator = find_child_by_field(node, "declarator")?;
    let name = find_identifier_in_declarator(declarator, source)?;
    let qualified_name = format!("{file_path}::{name}");

    // Check if this is a const-qualified declaration
    let text = get_node_text(node, source);
    let kind = if text.starts_with("const ") {
        NodeKind::Constant
    } else {
        NodeKind::Variable
    };

    let signature = Some(text.lines().next().unwrap_or("").to_string());

    Some(ExtractedNode {
        kind,
        name,
        qualified_name: Some(qualified_name),
        signature,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export: false,
        metadata: None,
    })
}

fn extract_calls(
    node: Node,
    source: &str,
    parent_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    if node.kind() == "call_expression" {
        if let Some(func) = find_child_by_field(node, "function") {
            let name = get_node_text(func, source).to_string();
            unresolved_refs.push(UnresolvedRef {
                source_idx: parent_idx,
                target_name: name,
                target_qualified_name: None,
                edge_kind: EdgeKind::Calls,
                import_path: None,
                context: None,
            });
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            extract_calls(child, source, parent_idx, unresolved_refs);
        }
    }
}

/// Detect environment variable reads: getenv("VAR")
fn extract_env_vars_c(
    source: &str,
    nodes: &mut Vec<ExtractedNode>,
    edges: &mut Vec<ExtractedEdge>,
) {
    let mut seen = HashSet::new();

    let pattern =
        regex_lite::Regex::new(r#"getenv\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#).unwrap();

    let first_func_idx = nodes.iter().position(|n| n.kind == NodeKind::Function);

    for cap in pattern.captures_iter(source) {
        let var_name = cap.get(1).unwrap().as_str();
        if !seen.insert(var_name.to_string()) {
            continue;
        }

        let env_idx = nodes.len();
        nodes.push(ExtractedNode {
            kind: NodeKind::EnvVar,
            name: var_name.to_string(),
            qualified_name: Some(format!("env::{var_name}")),
            signature: None,
            docstring: None,
            line_start: 0,
            line_end: 0,
            col_start: 0,
            col_end: 0,
            visibility: None,
            is_export: false,
            metadata: None,
        });

        if let Some(func_idx) = first_func_idx {
            edges.push(ExtractedEdge {
                source_idx: func_idx,
                target_idx: env_idx,
                kind: EdgeKind::ReadsEnv,
                confidence: 0.9,
                context: None,
            });
        }
    }
}
