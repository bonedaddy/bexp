use std::collections::HashMap;

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
        extract_from_node(
            root,
            source,
            file_path,
            &mut nodes,
            &mut edges,
            &mut unresolved_refs,
            None,
        );

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

/// Extract #[derive(...)] and #[cfg(...)] attributes from preceding attribute items.
fn extract_attributes(
    node: Node,
    source: &str,
    source_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) -> Option<HashMap<String, String>> {
    let mut metadata = HashMap::new();

    // Walk prev_sibling chain to collect attribute_item nodes
    let mut current = node.prev_sibling();
    while let Some(prev) = current {
        if prev.kind() == "attribute_item" {
            let text = get_node_text(prev, source);

            // Parse derive macros: #[derive(Trait1, Trait2)]
            if text.contains("derive") {
                if let Some(args_start) = text.find("derive(") {
                    let after = &text[args_start + 7..];
                    if let Some(end) = after.find(')') {
                        let traits_str = &after[..end];
                        let derives: Vec<&str> = traits_str
                            .split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .collect();

                        for trait_name in &derives {
                            unresolved_refs.push(UnresolvedRef {
                                source_idx,
                                target_name: trait_name.to_string(),
                                target_qualified_name: None,
                                edge_kind: EdgeKind::TypeRef,
                                import_path: None,
                                context: None,
                            });
                        }

                        metadata.insert(
                            "derive".to_string(),
                            derives.join(", "),
                        );
                    }
                }
            }

            // Parse cfg attributes: #[cfg(...)]
            if text.contains("cfg(") {
                if let Some(start) = text.find("cfg(") {
                    let after = &text[start + 4..];
                    if let Some(end) = after.rfind(')') {
                        metadata.insert("cfg".to_string(), after[..end].to_string());
                    }
                }
            }

            current = prev.prev_sibling();
        } else if prev.kind() == "line_comment" || prev.kind() == "block_comment" {
            // Skip comments between attributes
            current = prev.prev_sibling();
        } else {
            break;
        }
    }

    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

/// Extract generic type bounds from type_parameters and where clauses.
fn extract_generic_bounds(
    node: Node,
    source: &str,
    source_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    // Look for type_parameters child
    if let Some(type_params) = find_child_by_kind(node, "type_parameters") {
        for i in 0..type_params.child_count() {
            if let Some(child) = type_params.child(i as u32) {
                if child.kind() == "constrained_type_parameter" {
                    // Extract trait bounds: T: Trait
                    for j in 0..child.child_count() {
                        if let Some(bound_child) = child.child(j as u32) {
                            if bound_child.kind() == "trait_bound" {
                                if let Some(trait_type) =
                                    find_child_by_kind(bound_child, "type_identifier")
                                        .or_else(|| {
                                            find_child_by_kind(
                                                bound_child,
                                                "scoped_type_identifier",
                                            )
                                        })
                                        .or_else(|| find_child_by_kind(bound_child, "generic_type"))
                                {
                                    unresolved_refs.push(UnresolvedRef {
                                        source_idx,
                                        target_name: get_node_text(trait_type, source).to_string(),
                                        target_qualified_name: None,
                                        edge_kind: EdgeKind::TypeRef,
                                        import_path: None,
                                        context: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Look for where_clause
    if let Some(where_clause) = find_child_by_kind(node, "where_clause") {
        for i in 0..where_clause.child_count() {
            if let Some(pred) = where_clause.child(i as u32) {
                if pred.kind() == "where_predicate" {
                    for j in 0..pred.child_count() {
                        if let Some(bound_child) = pred.child(j as u32) {
                            if bound_child.kind() == "trait_bound" {
                                if let Some(trait_type) =
                                    find_child_by_kind(bound_child, "type_identifier")
                                        .or_else(|| {
                                            find_child_by_kind(
                                                bound_child,
                                                "scoped_type_identifier",
                                            )
                                        })
                                        .or_else(|| find_child_by_kind(bound_child, "generic_type"))
                                {
                                    unresolved_refs.push(UnresolvedRef {
                                        source_idx,
                                        target_name: get_node_text(trait_type, source).to_string(),
                                        target_qualified_name: None,
                                        edge_kind: EdgeKind::TypeRef,
                                        import_path: None,
                                        context: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Detect control flow context by walking up the AST from a call expression.
fn detect_call_context(node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "if_expression" | "match_expression" => return Some("conditional".into()),
            "for_expression" | "while_expression" | "loop_expression" => {
                return Some("loop".into())
            }
            "try_expression" => return Some("error_propagation".into()),
            // Stop at function boundary
            "function_item" | "closure_expression" => break,
            _ => {}
        }
        current = parent.parent();
    }
    None
}

fn push_contains_edge(
    edges: &mut Vec<ExtractedEdge>,
    parent: usize,
    child: usize,
) {
    edges.push(ExtractedEdge {
        source_idx: parent,
        target_idx: child,
        kind: EdgeKind::Contains,
        confidence: 1.0,
        context: None,
    });
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
            if let Some(extracted) = extract_function(node, source, file_path, parent_idx) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    push_contains_edge(edges, parent, idx);
                }
                extract_generic_bounds(node, source, idx, unresolved_refs);
                extract_calls(node, source, idx, unresolved_refs);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(
                            child,
                            source,
                            file_path,
                            nodes,
                            edges,
                            unresolved_refs,
                            Some(idx),
                        );
                    }
                }
                return;
            }
        }
        "struct_item" => {
            let expected_idx = nodes.len();
            if let Some(extracted) = extract_struct(node, source, file_path, expected_idx, unresolved_refs) {
                let idx = nodes.len();
                let struct_idx = idx;
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    push_contains_edge(edges, parent, idx);
                }
                extract_generic_bounds(node, source, struct_idx, unresolved_refs);
            }
        }
        "enum_item" => {
            if let Some(extracted) = extract_enum(node, source, file_path) {
                let idx = nodes.len();
                let enum_idx = idx;
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    push_contains_edge(edges, parent, idx);
                }
                // Extract enum variants
                extract_enum_variants(
                    node,
                    source,
                    file_path,
                    enum_idx,
                    nodes,
                    edges,
                    unresolved_refs,
                );
                extract_generic_bounds(node, source, enum_idx, unresolved_refs);
            }
        }
        "trait_item" => {
            if let Some(extracted) = extract_trait(node, source, file_path) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    push_contains_edge(edges, parent, idx);
                }
                extract_generic_bounds(node, source, idx, unresolved_refs);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(
                            child,
                            source,
                            file_path,
                            nodes,
                            edges,
                            unresolved_refs,
                            Some(idx),
                        );
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
                    push_contains_edge(edges, parent, idx);
                }
                // Check for trait implementation (AST-based, not text-based)
                extract_impl_trait_ref(node, source, idx, unresolved_refs);
                extract_generic_bounds(node, source, idx, unresolved_refs);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        extract_from_node(
                            child,
                            source,
                            file_path,
                            nodes,
                            edges,
                            unresolved_refs,
                            Some(idx),
                        );
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
                    push_contains_edge(edges, parent, idx);
                }
            }
        }
        "const_item" | "static_item" => {
            if let Some(extracted) = extract_constant(node, source, file_path) {
                let idx = nodes.len();
                nodes.push(extracted);
                if let Some(parent) = parent_idx {
                    push_contains_edge(edges, parent, idx);
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
                    push_contains_edge(edges, parent, idx);
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
                            Some(idx),
                        );
                    }
                }
                return;
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

fn extract_function(
    node: Node,
    source: &str,
    file_path: &str,
    parent_idx: Option<usize>,
) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")?;
    let name = get_node_text(name_node, source).to_string();

    let visibility = extract_visibility(node, source);

    let params = find_child_by_kind(node, "parameters")
        .map(|p| get_node_text(p, source).to_string())
        .unwrap_or_else(|| "()".to_string());

    let return_type = node
        .children(&mut node.walk())
        .find(|c| {
            c.kind() == "type_identifier"
                || c.kind() == "generic_type"
                || c.kind() == "reference_type"
                || c.kind() == "scoped_type_identifier"
        })
        .map(|t| format!(" -> {}", get_node_text(t, source)));

    let signature = format!("fn {}{}{}", name, params, return_type.unwrap_or_default());
    let qualified_name = format!("{}::{}", file_path, name);
    let is_pub = visibility.as_deref() == Some("pub");

    // Detect if this is a method (inside impl block, with self parameter)
    let is_method = parent_idx.is_some() && {
        if let Some(parent) = node.parent() {
            if parent.kind() == "impl_item" || parent.kind() == "declaration_list" {
                // Check first parameter for self/&self/&mut self
                if let Some(params_node) = find_child_by_kind(node, "parameters") {
                    has_self_param(params_node, source)
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    };

    let kind = if is_method {
        NodeKind::Method
    } else {
        NodeKind::Function
    };

    // Build metadata for methods
    let mut metadata = HashMap::new();
    if is_method {
        if let Some(params_node) = find_child_by_kind(node, "parameters") {
            if let Some(receiver) = get_receiver_type(params_node, source) {
                metadata.insert("receiver".to_string(), receiver);
            }
        }
    }

    // Extract attributes (cfg, etc.) — refs discarded since functions don't have derives
    let attr_metadata = extract_attributes(node, source, 0, &mut Vec::new());
    // Note: source_idx=0 is harmless here since unresolved_refs are discarded
    if let Some(attrs) = attr_metadata {
        metadata.extend(attrs);
    }

    let metadata = if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    };

    Some(ExtractedNode {
        kind,
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
        metadata,
    })
}

fn has_self_param(params_node: Node, source: &str) -> bool {
    if let Some(first_param) = params_node.child(1) {
        // child(0) is '(', child(1) is first parameter
        let kind = first_param.kind();
        if kind == "self_parameter" {
            return true;
        }
        let text = get_node_text(first_param, source);
        text == "self" || text == "&self" || text == "&mut self"
    } else {
        false
    }
}

fn get_receiver_type(params_node: Node, source: &str) -> Option<String> {
    if let Some(first_param) = params_node.child(1) {
        let text = get_node_text(first_param, source);
        if text.contains("self") {
            return Some(text.to_string());
        }
    }
    None
}

fn extract_struct(
    node: Node,
    source: &str,
    file_path: &str,
    expected_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let is_pub = visibility.as_deref() == Some("pub");

    // Build struct signature from fields (abbreviated to first 5)
    let signature = build_struct_signature(node, source, &name);

    // Extract attributes (derive, cfg) — use expected_idx so derive edges
    // point to this struct node, not node 0.
    let metadata = extract_attributes(node, source, expected_idx, unresolved_refs);

    Some(ExtractedNode {
        kind: NodeKind::Struct,
        name,
        qualified_name: Some(qualified_name),
        signature,
        docstring: get_preceding_comment(node, source),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        col_start: node.start_position().column,
        col_end: node.end_position().column,
        visibility,
        is_export: is_pub,
        metadata,
    })
}

fn build_struct_signature(node: Node, source: &str, name: &str) -> Option<String> {
    let field_list = find_child_by_kind(node, "field_declaration_list")?;
    let mut fields = Vec::new();

    for i in 0..field_list.child_count() {
        if let Some(child) = field_list.child(i as u32) {
            if child.kind() == "field_declaration" {
                let field_text = get_node_text(child, source).trim().to_string();
                fields.push(field_text);
            }
        }
    }

    if fields.is_empty() {
        return None;
    }

    let display_fields: Vec<&str> = fields.iter().take(5).map(|s| s.as_str()).collect();
    let suffix = if fields.len() > 5 { ", ..." } else { "" };
    Some(format!(
        "struct {} {{ {} {} }}",
        name,
        display_fields.join(", "),
        suffix
    ))
}

fn extract_enum(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let is_pub = visibility.as_deref() == Some("pub");

    // Extract attributes
    let metadata = extract_attributes(node, source, 0, &mut Vec::new());

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
        metadata,
    })
}

/// Extract enum variants as separate EnumVariant nodes with Contains edges.
fn extract_enum_variants(
    node: Node,
    source: &str,
    file_path: &str,
    enum_idx: usize,
    nodes: &mut Vec<ExtractedNode>,
    edges: &mut Vec<ExtractedEdge>,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    let variant_list = match find_child_by_kind(node, "enum_variant_list") {
        Some(vl) => vl,
        None => return,
    };

    let enum_name = nodes[enum_idx].name.clone();

    for i in 0..variant_list.child_count() {
        if let Some(child) = variant_list.child(i as u32) {
            if child.kind() == "enum_variant" {
                if let Some(name_node) = find_child_by_kind(child, "identifier") {
                    let variant_name = get_node_text(name_node, source).to_string();
                    let qualified_name =
                        format!("{}::{}::{}", file_path, enum_name, variant_name);

                    let idx = nodes.len();
                    nodes.push(ExtractedNode {
                        kind: NodeKind::EnumVariant,
                        name: variant_name,
                        qualified_name: Some(qualified_name),
                        signature: Some(get_node_text(child, source).trim().to_string()),
                        docstring: get_preceding_comment(child, source),
                        line_start: child.start_position().row + 1,
                        line_end: child.end_position().row + 1,
                        col_start: child.start_position().column,
                        col_end: child.end_position().column,
                        visibility: None,
                        is_export: false,
                        metadata: None,
                    });

                    push_contains_edge(edges, enum_idx, idx);

                    // Extract attributes for variants
                    extract_attributes(child, source, idx, unresolved_refs);
                }
            }
        }
    }
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
        metadata: None,
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
        metadata: None,
    })
}

/// Improved impl-trait detection using AST walk instead of text matching.
fn extract_impl_trait_ref(
    node: Node,
    source: &str,
    impl_idx: usize,
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    // Look for "for" keyword child between trait type and self type
    let mut found_impl = false;
    let mut has_for = false;
    let mut trait_name: Option<String> = None;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "impl" => found_impl = true,
                "for" => {
                    has_for = true;
                    // The trait name should have been captured before "for"
                    break;
                }
                "type_identifier" | "scoped_type_identifier" | "generic_type" if found_impl => {
                    if trait_name.is_none() {
                        trait_name = Some(get_node_text(child, source).to_string());
                    }
                }
                _ => {}
            }
        }
    }

    if has_for {
        if let Some(name) = trait_name {
            unresolved_refs.push(UnresolvedRef {
                source_idx: impl_idx,
                target_name: name,
                target_qualified_name: None,
                edge_kind: EdgeKind::Implements,
                import_path: None,
                context: None,
            });
        }
    }
}

fn extract_type_alias(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "type_identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
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
        visibility,
        is_export: false,
        metadata: None,
    })
}

fn extract_constant(node: Node, source: &str, file_path: &str) -> Option<ExtractedNode> {
    let name_node = find_child_by_kind(node, "identifier")?;
    let name = get_node_text(name_node, source).to_string();
    let qualified_name = format!("{}::{}", file_path, name);
    let visibility = extract_visibility(node, source);
    let signature = Some(
        get_node_text(node, source)
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
    );

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
        metadata: None,
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
        metadata: None,
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
        metadata: None,
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
                    context: None,
                });
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
            let context = detect_call_context(node);
            unresolved_refs.push(UnresolvedRef {
                source_idx: parent_idx,
                target_name: name,
                target_qualified_name: None,
                edge_kind: EdgeKind::Calls,
                import_path: None,
                context,
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
