use thiserror::Error;

#[derive(Error, Debug)]
pub enum VexpError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Parse error: {reason} in {file}")]
    Parse { file: String, reason: String },

    #[error("Index error: {0}")]
    Index(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Skeleton error: {0}")]
    Skeleton(String),

    #[error("Capsule error: {0}")]
    #[allow(dead_code)]
    Capsule(String),

    #[error("Memory error: {0}")]
    #[allow(dead_code)]
    Memory(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Token budget exceeded: requested {requested}, max {max}")]
    #[allow(dead_code)]
    BudgetExceeded { requested: usize, max: usize },
}

pub type Result<T> = std::result::Result<T, VexpError>;
