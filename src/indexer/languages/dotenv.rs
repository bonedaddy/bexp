use std::collections::HashMap;

use crate::indexer::extractor::{ExtractedFile, ExtractedNode};
use crate::types::{Language, NodeKind};

/// Parse .env files without tree-sitter.
/// Each KEY=value line becomes an EnvVar node.
pub fn extract_dotenv(source: &str, file_path: &str) -> ExtractedFile {
    let mut nodes = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse KEY=value
        if let Some((key, _value)) = trimmed.split_once('=') {
            let key = key.trim();
            if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            // Deduplicate: one node per unique key per file
            if !seen.insert(key.to_string()) {
                continue;
            }

            let mut metadata = HashMap::new();
            metadata.insert("defined_in".to_string(), file_path.to_string());

            nodes.push(ExtractedNode {
                kind: NodeKind::EnvVar,
                name: key.to_string(),
                qualified_name: Some(format!("env::{key}")),
                signature: Some(trimmed.to_string()),
                docstring: None,
                line_start: line_idx,
                line_end: line_idx,
                col_start: 0,
                col_end: line.len(),
                visibility: None,
                is_export: true,
                metadata: Some(metadata),
            });
        }
    }

    ExtractedFile {
        language: Language::Dotenv,
        content_hash: String::new(), // filled by caller
        mtime_ns: 0,                 // filled by caller
        size_bytes: 0,               // filled by caller
        nodes,
        edges: Vec::new(),
        unresolved_refs: Vec::new(),
        structure_hash: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_env_file() {
        let source = r#"
# Database config
DATABASE_URL=postgres://localhost/mydb
API_KEY=secret123

# Empty value
EMPTY_VAR=
# Comment
PORT=3000
"#;
        let result = extract_dotenv(source, ".env");
        assert_eq!(result.nodes.len(), 4);
        assert_eq!(result.nodes[0].name, "DATABASE_URL");
        assert_eq!(result.nodes[0].kind, NodeKind::EnvVar);
        assert_eq!(
            result.nodes[0].qualified_name.as_deref(),
            Some("env::DATABASE_URL")
        );
        assert_eq!(result.nodes[1].name, "API_KEY");
        assert_eq!(result.nodes[2].name, "EMPTY_VAR");
        assert_eq!(result.nodes[3].name, "PORT");

        // Check metadata
        let meta = result.nodes[0].metadata.as_ref().unwrap();
        assert_eq!(meta.get("defined_in"), Some(&".env".to_string()));
    }

    #[test]
    fn deduplicates_keys() {
        let source = "FOO=bar\nFOO=baz\n";
        let result = extract_dotenv(source, ".env");
        assert_eq!(result.nodes.len(), 1);
    }

    #[test]
    fn skips_invalid_keys() {
        let source = "=nokey\n   =bad\nGOOD_KEY=value\n";
        let result = extract_dotenv(source, ".env");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].name, "GOOD_KEY");
    }
}
