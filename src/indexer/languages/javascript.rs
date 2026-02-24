use tree_sitter::Tree;

use crate::indexer::extractor::*;
use crate::types::*;

/// JavaScript extractor delegates to TypeScript extractor since the AST
/// structure is nearly identical. The main difference is that JS doesn't
/// have type annotations, interfaces, or enums.
pub struct JavaScriptExtractor;

impl LanguageExtractor for JavaScriptExtractor {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn extract(&self, tree: &Tree, source: &str, file_path: &str) -> ExtractedFile {
        // Reuse the TypeScript extraction logic — it handles both
        let ts = super::typescript::TypeScriptExtractor;
        let mut result = ts.extract(tree, source, file_path);
        result.language = Language::JavaScript;
        result
    }
}
