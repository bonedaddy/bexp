use crossbeam_channel::{Receiver, Sender};

use crate::error::{BexpError, Result};
use crate::indexer::extractor::ExtractedFile;
use crate::indexer::languages;
use crate::types::Language;

pub struct ParserPool {
    sender: Sender<tree_sitter::Parser>,
    receiver: Receiver<tree_sitter::Parser>,
}

impl Default for ParserPool {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserPool {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
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
        let mut parser = self
            .receiver
            .try_recv()
            .unwrap_or_else(|_| tree_sitter::Parser::new());

        // Dotenv files are parsed without tree-sitter; handled before calling parse()
        let ts_language = match lang {
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Html => tree_sitter_html::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Language::Dotenv => {
                return Err(BexpError::Parse {
                    file: file_path.to_string(),
                    reason: "Dotenv files should not be parsed via tree-sitter".to_string(),
                });
            }
        };

        let result = (|| {
            parser
                .set_language(&ts_language)
                .map_err(|e| BexpError::Parse {
                    file: file_path.to_string(),
                    reason: format!("Language setup failed: {e}"),
                })?;

            let tree = parser.parse(source, None).ok_or_else(|| BexpError::Parse {
                file: file_path.to_string(),
                reason: "Parse returned None".to_string(),
            })?;

            let extractor = languages::get_extractor(lang);
            let mut extracted = extractor.extract(&tree, source, file_path);
            extracted.content_hash = content_hash;
            extracted.mtime_ns = mtime_ns;
            extracted.size_bytes = size_bytes;

            Ok(extracted)
        })();

        // Always return parser to pool for reuse
        let _ = self.sender.send(parser);

        result
    }
}
