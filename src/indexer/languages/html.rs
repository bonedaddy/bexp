use tree_sitter::{Node, Tree};

use crate::indexer::extractor::*;
use crate::types::*;

pub struct HtmlExtractor;

impl LanguageExtractor for HtmlExtractor {
    fn language(&self) -> Language {
        Language::Html
    }

    fn extract(&self, tree: &Tree, source: &str, file_path: &str) -> ExtractedFile {
        let mut nodes = Vec::new();
        let mut unresolved_refs = Vec::new();

        let root = tree.root_node();
        extract_from_node(root, source, file_path, &mut nodes, &mut unresolved_refs);

        ExtractedFile {
            language: Language::Html,
            content_hash: String::new(),
            mtime_ns: 0,
            size_bytes: 0,
            nodes,
            edges: Vec::new(),
            unresolved_refs,
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

fn get_tag_name(node: Node, source: &str) -> Option<String> {
    find_child_by_kind(node, "start_tag")
        .and_then(|tag| find_child_by_kind(tag, "tag_name"))
        .map(|n| get_node_text(n, source).to_string())
}

fn get_attribute_value(node: Node, source: &str, attr_name: &str) -> Option<String> {
    let start_tag = find_child_by_kind(node, "start_tag")?;
    for i in 0..start_tag.child_count() {
        if let Some(child) = start_tag.child(i) {
            if child.kind() == "attribute" {
                if let Some(name_node) = find_child_by_kind(child, "attribute_name") {
                    if get_node_text(name_node, source) == attr_name {
                        return find_child_by_kind(child, "quoted_attribute_value")
                            .map(|v| {
                                let text = get_node_text(v, source);
                                text.trim_matches('"').trim_matches('\'').to_string()
                            });
                    }
                }
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
    unresolved_refs: &mut Vec<UnresolvedRef>,
) {
    match node.kind() {
        "script_element" => {
            // Script with src attribute is an import; inline script is ignored for extraction
            if let Some(src) = get_attribute_value(node, source, "src") {
                let idx = nodes.len();
                nodes.push(ExtractedNode {
                    kind: NodeKind::Import,
                    name: src.clone(),
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
                unresolved_refs.push(UnresolvedRef {
                    source_idx: idx,
                    target_name: src,
                    target_qualified_name: None,
                    edge_kind: EdgeKind::Imports,
                    import_path: None,
                });
            }
        }
        "style_element" => {
            nodes.push(ExtractedNode {
                kind: NodeKind::Module,
                name: "<style>".to_string(),
                qualified_name: Some(format!("{}::<style>", file_path)),
                signature: None,
                docstring: None,
                line_start: node.start_position().row + 1,
                line_end: node.end_position().row + 1,
                col_start: node.start_position().column,
                col_end: node.end_position().column,
                visibility: None,
                is_export: false,
            });
        }
        "element" => {
            if let Some(tag) = get_tag_name(node, source) {
                if tag == "link" {
                    if let Some(href) = get_attribute_value(node, source, "href") {
                        let idx = nodes.len();
                        nodes.push(ExtractedNode {
                            kind: NodeKind::Import,
                            name: href.clone(),
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
                        unresolved_refs.push(UnresolvedRef {
                            source_idx: idx,
                            target_name: href,
                            target_qualified_name: None,
                            edge_kind: EdgeKind::Imports,
                            import_path: None,
                        });
                    }
                }
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            extract_from_node(child, source, file_path, nodes, unresolved_refs);
        }
    }
}
