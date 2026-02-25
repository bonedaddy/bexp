/// Pre-tokenize code identifiers for FTS5 search.
///
/// Splits camelCase, snake_case, and qualified names into space-separated
/// lowercase tokens while preserving the original for exact-match fallback.
///
/// Examples:
/// - `"parseConfigFile"` → `"parse config file parseconfigfile"`
/// - `"get_node_text"`   → `"get node text get_node_text"`
/// - `"GraphEngine"`     → `"graph engine graphengine"`
/// - `"bfs_direction"`   → `"bfs direction bfs_direction"`
pub fn tokenize_identifier(name: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Split on common separators: qualified name (::, .), hyphen, slash
    for segment in name.split([':', '.', '-', '/']) {
        if segment.is_empty() {
            continue;
        }
        // Split each segment on underscores
        for sub in segment.split('_') {
            if sub.is_empty() {
                continue;
            }
            // Split camelCase
            split_camel_case(sub, &mut parts);
        }
    }

    if parts.is_empty() {
        return name.to_lowercase();
    }

    // Lowercase all parts
    let tokens: Vec<String> = parts.iter().map(|p| p.to_lowercase()).collect();

    // Append original (lowercased) for exact-match fallback
    let original_lower = name.to_lowercase();
    let mut result = tokens.join(" ");
    if !result.contains(&original_lower) {
        result.push(' ');
        result.push_str(&original_lower);
    }

    result
}

fn split_camel_case(s: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return;
    }

    let mut current = String::new();
    current.push(chars[0]);

    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let cur = chars[i];

        let is_boundary =
            // lowercase→uppercase boundary (e.g., "parseConfig" → "parse" + "Config")
            (prev.is_lowercase() && cur.is_uppercase())
            // uppercase→uppercase→lowercase boundary (e.g., "HTMLParser" → "HTML" + "Parser")
            || (i + 1 < chars.len()
                && prev.is_uppercase()
                && cur.is_uppercase()
                && chars[i + 1].is_lowercase());

        if is_boundary {
            out.push(current);
            current = String::new();
        }

        current.push(cur);
    }

    if !current.is_empty() {
        out.push(current);
    }
}

/// Tokenize a search query so FTS5 can match against tokenized columns.
/// Also strips FTS5 special characters (-, :, *, ", ^) to prevent query injection.
pub fn tokenize_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|w| {
            let tokenized = tokenize_identifier(w);
            // Strip FTS5 special characters from the tokenized result
            // to prevent query syntax injection (hyphens, colons, quotes, etc.)
            tokenized
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_splitting() {
        assert_eq!(
            tokenize_identifier("parseConfigFile"),
            "parse config file parseconfigfile"
        );
    }

    #[test]
    fn snake_case_splitting() {
        assert_eq!(
            tokenize_identifier("get_node_text"),
            "get node text get_node_text"
        );
    }

    #[test]
    fn pascal_case_splitting() {
        assert_eq!(
            tokenize_identifier("GraphEngine"),
            "graph engine graphengine"
        );
    }

    #[test]
    fn mixed_snake_camel() {
        assert_eq!(
            tokenize_identifier("bfs_direction"),
            "bfs direction bfs_direction"
        );
    }

    #[test]
    fn qualified_name_splitting() {
        let result = tokenize_identifier("src/db/queries.rs::insertNode");
        assert!(result.contains("insert"));
        assert!(result.contains("node"));
        assert!(result.contains("queries"));
    }

    #[test]
    fn acronym_splitting() {
        assert_eq!(
            tokenize_identifier("HTMLParser"),
            "html parser htmlparser"
        );
    }

    #[test]
    fn single_word() {
        assert_eq!(tokenize_identifier("parse"), "parse");
    }

    #[test]
    fn empty_string() {
        assert_eq!(tokenize_identifier(""), "");
    }

    #[test]
    fn query_tokenization() {
        let result = tokenize_query("parseConfig error handler");
        assert!(result.contains("parse"));
        assert!(result.contains("config"));
        assert!(result.contains("error"));
        assert!(result.contains("handler"));
    }
}
