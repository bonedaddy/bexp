use std::collections::HashMap;
use std::fmt::Write;

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
    pub structure_hash: Option<String>,
}

/// Compute a deterministic hash of the structural shape of a file's nodes and edges.
/// Format-only changes (whitespace, comments) that don't alter the AST structure
/// will produce the same hash.
pub fn compute_structure_hash(nodes: &[ExtractedNode], edges: &[ExtractedEdge]) -> String {
    let mut data = String::new();
    // Sort nodes by (kind, name) for determinism
    let mut node_keys: Vec<_> = nodes
        .iter()
        .map(|n| {
            (
                n.kind.as_str(),
                n.name.as_str(),
                n.signature.as_deref().unwrap_or(""),
                n.line_start,
                n.line_end,
            )
        })
        .collect();
    node_keys.sort();
    for (kind, name, sig, ls, le) in &node_keys {
        let _ = write!(data, "{kind}:{name}:{sig}:{ls}:{le};");
    }
    // Sort edges by (source_idx, target_idx, kind)
    let mut edge_keys: Vec<_> = edges
        .iter()
        .map(|e| (e.source_idx, e.target_idx, e.kind.as_str()))
        .collect();
    edge_keys.sort();
    for (si, ti, kind) in &edge_keys {
        let _ = write!(data, "e:{si}:{ti}:{kind};");
    }
    let hash = xxhash_rust::xxh3::xxh3_64(data.as_bytes());
    format!("{hash:016x}")
}

pub trait LanguageExtractor: Send + Sync {
    fn extract(&self, tree: &tree_sitter::Tree, source: &str, file_path: &str) -> ExtractedFile;

    #[allow(dead_code)]
    fn language(&self) -> Language;
}
