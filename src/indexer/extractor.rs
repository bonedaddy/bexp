use std::collections::HashMap;

use crate::types::{EdgeKind, Language, NodeKind};

#[derive(Debug, Clone)]
pub struct ExtractedNode {
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: Option<String>,
    pub signature: Option<String>,
    pub docstring: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub visibility: Option<String>,
    pub is_export: bool,
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct ExtractedEdge {
    pub source_idx: usize,
    pub target_idx: usize,
    pub kind: EdgeKind,
    pub confidence: f64,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnresolvedRef {
    pub source_idx: usize,
    pub target_name: String,
    pub target_qualified_name: Option<String>,
    pub edge_kind: EdgeKind,
    pub import_path: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedFile {
    pub language: Language,
    pub content_hash: String,
    pub mtime_ns: i64,
    pub size_bytes: u64,
    pub nodes: Vec<ExtractedNode>,
    pub edges: Vec<ExtractedEdge>,
    pub unresolved_refs: Vec<UnresolvedRef>,
}

pub trait LanguageExtractor: Send + Sync {
    fn extract(
        &self,
        tree: &tree_sitter::Tree,
        source: &str,
        file_path: &str,
    ) -> ExtractedFile;

    fn language(&self) -> Language;
}
