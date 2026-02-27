use crate::types::{DetailLevel, Language};

pub struct SkeletonRules {
    /// Node kinds whose bodies should be collapsed
    pub collapsible: Vec<&'static str>,
    /// Mapping from node kind to its body child kind
    pub body_kinds: Vec<(&'static str, &'static str)>,
    /// Node kinds to fully remove at minimal level
    pub removable: Vec<&'static str>,
    /// Child kinds that are "significant" (kept as stubs in standard level)
    pub significant_children: Vec<&'static str>,
}

impl SkeletonRules {
    pub fn should_collapse_body(&self, kind: &str, level: DetailLevel) -> bool {
        match level {
            DetailLevel::Minimal | DetailLevel::Standard => self.collapsible.contains(&kind),
            DetailLevel::Detailed => false,
        }
    }

    pub fn body_kind(&self, kind: &str) -> Option<&'static str> {
        self.body_kinds
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| *v)
    }

    pub fn should_remove(&self, kind: &str) -> bool {
        self.removable.contains(&kind)
    }

    pub fn is_significant_child(&self, kind: &str) -> bool {
        self.significant_children.contains(&kind)
    }
}

pub fn get_rules(lang: Language) -> SkeletonRules {
    match lang {
        Language::TypeScript | Language::JavaScript => SkeletonRules {
            collapsible: vec![
                "function_declaration",
                "method_definition",
                "arrow_function",
                "function",
                "generator_function_declaration",
            ],
            body_kinds: vec![
                ("function_declaration", "statement_block"),
                ("method_definition", "statement_block"),
                ("arrow_function", "statement_block"),
                ("function", "statement_block"),
                ("generator_function_declaration", "statement_block"),
            ],
            removable: vec!["comment"],
            significant_children: vec![
                "function_declaration",
                "method_definition",
                "class_declaration",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "lexical_declaration",
                "variable_declaration",
                "export_statement",
            ],
        },
        Language::Python => SkeletonRules {
            collapsible: vec!["function_definition", "class_definition"],
            body_kinds: vec![
                ("function_definition", "block"),
                ("class_definition", "block"),
            ],
            removable: vec!["comment"],
            significant_children: vec![
                "function_definition",
                "class_definition",
                "assignment",
                "import_statement",
                "import_from_statement",
                "decorated_definition",
            ],
        },
        Language::Rust => SkeletonRules {
            collapsible: vec!["function_item", "impl_item"],
            body_kinds: vec![
                ("function_item", "block"),
                ("impl_item", "declaration_list"),
            ],
            removable: vec!["line_comment", "block_comment"],
            significant_children: vec![
                "function_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "impl_item",
                "type_item",
                "const_item",
                "static_item",
                "use_declaration",
                "mod_item",
            ],
        },
        Language::Html => SkeletonRules {
            collapsible: vec!["script_element", "style_element"],
            body_kinds: vec![
                ("script_element", "raw_text"),
                ("style_element", "raw_text"),
            ],
            removable: vec!["comment"],
            significant_children: vec!["element", "script_element", "style_element", "doctype"],
        },
        Language::C => SkeletonRules {
            collapsible: vec!["function_definition"],
            body_kinds: vec![("function_definition", "compound_statement")],
            removable: vec!["comment"],
            significant_children: vec![
                "function_definition",
                "struct_specifier",
                "enum_specifier",
                "type_definition",
                "preproc_include",
                "declaration",
            ],
        },
        Language::Dotenv => SkeletonRules {
            collapsible: vec![],
            body_kinds: vec![],
            removable: vec![],
            significant_children: vec![],
        },
        Language::Cpp => SkeletonRules {
            collapsible: vec!["function_definition", "namespace_definition"],
            body_kinds: vec![
                ("function_definition", "compound_statement"),
                ("namespace_definition", "declaration_list"),
            ],
            removable: vec!["comment"],
            significant_children: vec![
                "function_definition",
                "struct_specifier",
                "enum_specifier",
                "type_definition",
                "preproc_include",
                "declaration",
                "class_specifier",
                "template_declaration",
                "namespace_definition",
                "alias_declaration",
                "using_declaration",
            ],
        },
    }
}
