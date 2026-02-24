use crate::error::{Result, VexpError};
use crate::indexer::extractor::ExtractedFile;
use crate::indexer::languages;
use crate::types::Language;

pub struct ParserPool;

impl ParserPool {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(
        &self,
        source: &str,
        lang: Language,
        file_path: &str,
        content_hash: String,
        mtime_ns: i64,
        size_bytes: u64,
    ) -> Result<ExtractedFile> {
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
            VexpError::Parse {
                file: file_path.to_string(),
                reason: format!("Language setup failed: {e}"),
            }
        })?;

        let tree = parser.parse(source, None).ok_or_else(|| VexpError::Parse {
            file: file_path.to_string(),
            reason: "Parse returned None".to_string(),
        })?;

        let extractor = languages::get_extractor(lang);
        let mut extracted = extractor.extract(&tree, source, file_path);
        extracted.content_hash = content_hash;
        extracted.mtime_ns = mtime_ns;
        extracted.size_bytes = size_bytes;

        Ok(extracted)
    }
}
