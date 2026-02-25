use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Enum,
    EnumVariant,
    TypeAlias,
    Trait,
    Impl,
    Module,
    Variable,
    Constant,
    Import,
    External,
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::EnumVariant => "enum_variant",
            Self::TypeAlias => "type_alias",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Module => "module",
            Self::Variable => "variable",
            Self::Constant => "constant",
            Self::Import => "import",
            Self::External => "external",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "function" => Some(Self::Function),
            "method" => Some(Self::Method),
            "class" => Some(Self::Class),
            "struct" => Some(Self::Struct),
            "interface" => Some(Self::Interface),
            "enum" => Some(Self::Enum),
            "enum_variant" => Some(Self::EnumVariant),
            "type_alias" => Some(Self::TypeAlias),
            "trait" => Some(Self::Trait),
            "impl" => Some(Self::Impl),
            "module" => Some(Self::Module),
            "variable" => Some(Self::Variable),
            "constant" => Some(Self::Constant),
            "import" => Some(Self::Import),
            "external" => Some(Self::External),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Calls,
    Imports,
    Implements,
    Extends,
    TypeRef,
    Contains,
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::Imports => "imports",
            Self::Implements => "implements",
            Self::Extends => "extends",
            Self::TypeRef => "type_ref",
            Self::Contains => "contains",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "calls" => Some(Self::Calls),
            "imports" => Some(Self::Imports),
            "implements" => Some(Self::Implements),
            "extends" => Some(Self::Extends),
            "type_ref" => Some(Self::TypeRef),
            "contains" => Some(Self::Contains),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    Minimal,
    Standard,
    Detailed,
}

impl DetailLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Detailed => "detailed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "minimal" => Some(Self::Minimal),
            "standard" => Some(Self::Standard),
            "detailed" => Some(Self::Detailed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Debug,
    BlastRadius,
    Modify,
    Explore,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::BlastRadius => "blast_radius",
            Self::Modify => "modify",
            Self::Explore => "explore",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    TypeScript,
    JavaScript,
    Python,
    Rust,
    Html,
    C,
    Cpp,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Html => "html",
            Self::C => "c",
            Self::Cpp => "cpp",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "py" | "pyi" => Some(Self::Python),
            "rs" => Some(Self::Rust),
            "html" | "htm" => Some(Self::Html),
            "c" | "h" => Some(Self::C),
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" => Some(Self::Cpp),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_extension_supports_known_aliases() {
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("jsx"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("pyi"), Some(Language::Python));
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("htm"), Some(Language::Html));
        assert_eq!(Language::from_extension("h"), Some(Language::C));
        assert_eq!(Language::from_extension("hpp"), Some(Language::Cpp));
        assert_eq!(Language::from_extension("txt"), None);
    }

    #[test]
    fn node_kind_roundtrips_through_string_representation() {
        let all = [
            NodeKind::Function,
            NodeKind::Method,
            NodeKind::Class,
            NodeKind::Struct,
            NodeKind::Interface,
            NodeKind::Enum,
            NodeKind::EnumVariant,
            NodeKind::TypeAlias,
            NodeKind::Trait,
            NodeKind::Impl,
            NodeKind::Module,
            NodeKind::Variable,
            NodeKind::Constant,
            NodeKind::Import,
            NodeKind::External,
        ];

        for kind in all {
            assert_eq!(NodeKind::parse(kind.as_str()), Some(kind));
            // Verify Display matches as_str
            assert_eq!(kind.to_string(), kind.as_str());
        }

        assert_eq!(NodeKind::parse("not_a_kind"), None);
    }

    #[test]
    fn edge_kind_roundtrips_through_string_representation() {
        let all = [
            EdgeKind::Calls,
            EdgeKind::Imports,
            EdgeKind::Implements,
            EdgeKind::Extends,
            EdgeKind::TypeRef,
            EdgeKind::Contains,
        ];

        for kind in all {
            assert_eq!(EdgeKind::parse(kind.as_str()), Some(kind));
        }

        assert_eq!(EdgeKind::parse("not_an_edge"), None);
    }

    #[test]
    fn detail_level_roundtrips_through_string_representation() {
        let all = [DetailLevel::Minimal, DetailLevel::Standard, DetailLevel::Detailed];

        for level in all {
            assert_eq!(DetailLevel::parse(level.as_str()), Some(level));
        }

        assert_eq!(DetailLevel::parse("unknown"), None);
    }
}
