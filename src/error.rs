use thiserror::Error;

#[derive(Error, Debug)]
pub enum BexpError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Parse error: {reason} in {file}")]
    Parse { file: String, reason: String },

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Skeleton error: {0}")]
    Skeleton(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("File too large: {path} is {size} bytes (max {max})")]
    FileTooLarge { path: String, size: u64, max: u64 },

    #[error("Lock poisoned in {component}")]
    LockPoisoned { component: String },

    #[error("Unsupported language for extension: {extension}")]
    UnsupportedLanguage { extension: String },
}

pub type Result<T> = std::result::Result<T, BexpError>;
