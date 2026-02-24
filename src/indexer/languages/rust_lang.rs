use tree_sitter::{Node, Tree};

use crate::indexer::extractor::*;
use crate::types::*;

pub struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extract(&self, tree: &Tree, source: &str, file_path: &str) -> ExtractedFile {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut unresolved_refs = Vec::new();

        let root = tree.root_node();
        extract_from_node(root, source, file_path, &mut nodes, &mut edges, &mut unresolved_refs, None);

        ExtractedFile {
            language: Language::Rust,
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

fn get_preceding_comment(node: Node, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(prev) = current {
        if prev.kind() == "line_comment" || prev.kind() == "block_comment" {
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

fn extract_visibility(node: Node, source: &str) -> Option<String> {
    find_child_by_kind(node, "visibility_modifier")
        .map(|v| get_node_text(v, source).to_string())
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
        "function_item" => {
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
        "struct_item" => {
            if let Some(extracted) = extract_struct(node, source, file_path) {
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
        "enum_item" => {
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
        "trait_item" => {
            if let Some(extracted) = extract_trait(node, source, file_path) {
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
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
            }
        }
        "impl_item" => {
            if let Some(extracted) = extract_impl(node, source, file_path) {
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
                // Check for trait implementation
                extract_impl_trait_ref(node, source, idx, unresolved_refs);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
            }
        }
        "type_item" => {
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
        "const_item" | "static_item" => {
            if let Some(extracted) = extract_constant(node, source, file_path) {
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
        "use_declaration" => {
            extract_use(node, source, file_path, nodes, unresolved_refs);
        }
        "mod_item" => {
            if let Some(extracted) = extract_module(node, source, file_path) {
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
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(child, source, file_path, nodes, edges, unresolved_refs, Some(idx));
                    }
                }
                return;
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

    let visibility = extract_visibility(node, source);

    let params = find_child_by_kind(node, "parameters")
        .map(|p| get_node_text(p, source).to_string())
        .unwrap_or_else(|| "()".to_string());

    let return_type = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "type_identifier" || c.kind() == "generic_type" || c.kind() == "reference_type" || c.kind() == "scoped_type_identifier")
        .map(|t| format!(" -> {}", get_node_text(t, source)));

    let signature = format!("fn {}{}{}", name, params, return_type.unwrap_or_default());
    let qualified_name = format!("{}::{}", file_path, name);
    let is_pub = visibility.as_deref() == Some("pub");

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
        visibility,
        is_export: is_pub,
    })
}

fn extract_struct(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let is_pub = visibility.as_deref() == Some("pub");

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
        visibility,
        is_export: is_pub,
    })
}

fn extract_enum(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let is_pub = visibility.as_deref() == Some("pub");

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
        visibility,
        is_export: is_pub,
    })
}

fn extract_trait(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let is_pub = visibility.as_deref() == Some("pub");

    Some(ExtractedNode {
        kind: NodeKind::Trait,
        name,
        qualified_name: Some(qualified_name),
        signature: None,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility,
        is_export: is_pub,
    })
}

fn extract_impl(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let type_node = find_child_by_kind(node, "type_identifier")
        .or_else(|| find_child_by_kind(node, "generic_type"))?;
    let name = get_node_text(type_node, source).to_string();
    let qualified_name = format!("{}::impl_{}", file_path, name);

    Some(ExtractedNode {
        kind: NodeKind::Impl,
        name: format!("impl {}", name),
        qualified_name: Some(qualified_name),
        signature: None,
        docstring: None,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility: None,
        is_export: false,
    })
}

fn extract_impl_trait_ref(
    node: Node,
    source: &str,
    impl_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    // Look for "impl Trait for Type" pattern
    let text = get_node_text(node, source);
    if text.contains(" for ") {
        // The trait name is typically the first type_identifier after "impl"
        let mut found_impl = false;
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "impl" {
                    found_impl = true;
                    continue;
                }
                if found_impl && (child.kind() == "type_identifier" || child.kind() == "scoped_type_identifier") {
                    let trait_name = get_node_text(child, source).to_string();
                    unresolved_refs.push(UnresolvedRef {
                        source_idx: impl_idx,
                        target_name: trait_name,
                        target_qualified_name: None,
                        edge_kind: EdgeKind::Implements,
                        import_path: None,
                    });
                    break;
                }
            }
        }
    }
}

fn extract_type_alias(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let signature = Some(get_node_text(node, source).lines().next().unwrap_or("").to_string());

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
        visibility,
        is_export: false,
    })
}

fn extract_constant(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let signature = Some(get_node_text(node, source).lines().next().unwrap_or("").to_string());

    Some(ExtractedNode {
        kind: NodeKind::Constant,
        name,
        qualified_name: Some(qualified_name),
        signature,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility,
        is_export: false,
    })
}

fn extract_module(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);

    Some(ExtractedNode {
        kind: NodeKind::Module,
        name,
        qualified_name: Some(qualified_name),
        signature: None,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility,
        is_export: false,
    })
}

fn extract_use(
    node: Node,
    source: &str,
    _file_path: &str,
    nodes: &mut Vec<ExtractedNode>,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    let text = get_node_text(node, source).to_string();

    let idx = nodes.len();
    nodes.push(ExtractedNode {
        kind: NodeKind::Import,
        name: text.clone(),
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

    // Extract the use path components
    fn collect_use_names(node: Node, source: &str, idx: usize, refs: &mut Vec<UnresolvedRef>) {
        match node.kind() {
            "identifier" => {
                let name = get_node_text(node, source).to_string();
                refs.push(UnresolvedRef {
                    source_idx: idx,
                    target_name: name,
                    target_qualified_name: None,
                    edge_kind: EdgeKind::Imports,
                    import_path: None,
                });
            }
            "use_list" | "use_group" => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        collect_use_names(child, source, idx, refs);
                    }
                }
            }
            "scoped_identifier" | "use_as_clause" | "scoped_use_list" => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        collect_use_names(child, source, idx, refs);
                    }
                }
            }
            _ => {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        collect_use_names(child, source, idx, refs);
                    }
                }
            }
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() != "use" && child.kind() != "visibility_modifier" {
                collect_use_names(child, source, idx, unresolved_refs);
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
            if child.kind() == "closure_expression" {
                continue;
            }
            extract_calls(child, source, parent_idx, unresolved_refs);
        }
    }
}
