use crate::error::{Result, VexpError};
use crate::types::{DetailLevel, Language};

pub struct SkeletonTransformer;

impl SkeletonTransformer {
    pub fn transform(source: &str, lang: Language, level: DetailLevel) -> Result<String> {
        let mut parser = tree_sitter::Parser::new();

        let ts_language = match lang {
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Html => tree_sitter_html::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        };

        parser.set_language(&ts_language).map_err(|e| {
            VexpError::Skeleton(format!("Language setup failed: {e}"))
        })?;

        let tree = parser.parse(source, None).ok_or_else(|| {
            VexpError::Skeleton("Parse returned None".to_string())
        })?;

        let rules = super::languages::get_rules(lang);
        let skeleton = transform_node(tree.root_node(), source, level, &rules, 0);

        Ok(skeleton)
    }
}

fn transform_node(
    node: tree_sitter::Node,
    source: &str,
    level: DetailLevel,
    rules: &super::languages::SkeletonRules,
    depth: usize,
) -> String {
    let kind = node.kind();
    let text = &source[node.byte_range()];

    // Check if this node type should have its body collapsed
    if rules.should_collapse_body(kind, level) {
        if let Some(collapsed) = collapse_body(node, source, level, rules) {
            return collapsed;
        }
    }

    // For nodes that should be fully removed at minimal level
    if level == DetailLevel::Minimal && rules.should_remove(kind) {
        return String::new();
    }

    // If this is a leaf node or a node we don't need to process children for
    if node.child_count() == 0 {
        return text.to_string();
    }

    // Reconstruct from children
    let mut result = String::new();
    let mut last_end = node.start_byte();

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            // Add any whitespace/text between children
            if child.start_byte() > last_end {
                result.push_str(&source[last_end..child.start_byte()]);
            }

            let child_text = transform_node(child, source, level, rules, depth + 1);
            result.push_str(&child_text);
            last_end = child.end_byte();
        }
    }

    // Add trailing text
    if node.end_byte() > last_end {
        result.push_str(&source[last_end..node.end_byte()]);
    }

    result
}

fn collapse_body(
    node: tree_sitter::Node,
    source: &str,
    level: DetailLevel,
    rules: &super::languages::SkeletonRules,
) -> Option<String> {
    let kind = node.kind();

    // Find the body/block child
    let body_kind = rules.body_kind(kind)?;

    let mut body_node = None;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == body_kind {
                body_node = Some(child);
                break;
            }
        }
    }

    let body = body_node?;

    // Get the signature part (everything before the body)
    let sig_text = &source[node.start_byte()..body.start_byte()];

    match level {
        DetailLevel::Minimal => {
            // Just signature + { ... }
            Some(format!("{}{}", sig_text.trim_end(), " { ... }"))
        }
        DetailLevel::Standard => {
            // Signature + first-level items as stubs
            let _body_text = &source[body.byte_range()];
            let first_line_items = extract_first_level_stubs(body, source, rules);
            if first_line_items.is_empty() {
                Some(format!("{}{}", sig_text.trim_end(), " { ... }"))
            } else {
                Some(format!("{}{{\n{}\n}}", sig_text, first_line_items))
            }
        }
        DetailLevel::Detailed => {
            // Keep comments and first two levels
            Some(format!(
                "{}{}",
                sig_text,
                collapse_body_detailed(body, source, rules)
            ))
        }
    }
}

fn extract_first_level_stubs(
    body_node: tree_sitter::Node,
    source: &str,
    rules: &super::languages::SkeletonRules,
) -> String {
    let mut stubs = Vec::new();

    for i in 0..body_node.child_count() {
        if let Some(child) = body_node.child(i as u32) {
            let kind = child.kind();

            // Skip braces and delimiters
            if kind == "{" || kind == "}" || kind == ":" {
                continue;
            }

            if rules.is_significant_child(kind) {
                // Get just the signature line
                let text = &source[child.byte_range()];
                let first_line = text.lines().next().unwrap_or("");
                stubs.push(format!("    {}", first_line));
            }
        }
    }

    stubs.join("\n")
}

fn collapse_body_detailed(
    body_node: tree_sitter::Node,
    source: &str,
    rules: &super::languages::SkeletonRules,
) -> String {
    let _text = &source[body_node.byte_range()];

    // For detailed, keep most content but collapse deeply nested function bodies
    let mut result = String::new();
    let mut last_end = body_node.start_byte();

    for i in 0..body_node.child_count() {
        if let Some(child) = body_node.child(i as u32) {
            if child.start_byte() > last_end {
                result.push_str(&source[last_end..child.start_byte()]);
            }

            let child_kind = child.kind();
            if rules.should_collapse_body(child_kind, DetailLevel::Minimal) {
                // Collapse nested bodies
                if let Some(collapsed) =
                    collapse_body(child, source, DetailLevel::Standard, rules)
                {
                    result.push_str(&collapsed);
                } else {
                    result.push_str(&source[child.byte_range()]);
                }
            } else {
                result.push_str(&source[child.byte_range()]);
            }

            last_end = child.end_byte();
        }
    }

    if body_node.end_byte() > last_end {
        result.push_str(&source[last_end..body_node.end_byte()]);
    }

    result
}
