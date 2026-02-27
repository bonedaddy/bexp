use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{BexpError, Result};
use crate::types::DetailLevel;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BexpConfig {
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,

    #[serde(default = "default_skeleton_level")]
    pub default_skeleton_level: DetailLevel,

    #[serde(default)]
    pub exclude_patterns: Vec<String>,

    #[serde(default = "default_db_path")]
    pub db_path: String,

    #[serde(default = "default_max_file_size")]
    pub max_file_size: usize,

    #[serde(default = "default_watcher_debounce_ms")]
    pub watcher_debounce_ms: u64,

    /// Percentage of token budget reserved for memory/observation context (0..100).
    #[serde(default = "default_memory_budget_pct")]
    pub memory_budget_pct: usize,

    #[serde(default = "default_session_compress_after_hours")]
    pub session_compress_after_hours: u64,

    #[serde(default = "default_observation_ttl_days")]
    pub observation_ttl_days: u64,

    #[serde(default)]
    pub lsp_resolution: bool,

    #[serde(default)]
    pub lsp_servers: HashMap<String, LspServerConfig>,

    #[serde(default)]
    pub workspace_group: Vec<String>,

    #[serde(default = "default_reader_pool_size")]
    pub reader_pool_size: usize,

    #[serde(default = "default_capsule_cache_size")]
    pub capsule_cache_size: usize,

    #[serde(default = "default_capsule_cache_ttl_secs")]
    pub capsule_cache_ttl_secs: u64,

    #[serde(default)]
    pub health_port: Option<u16>,

    #[serde(default = "default_shutdown_drain_secs")]
    pub shutdown_drain_secs: u64,

    /// Context lines to pad around each node range in capsule excerpts.
    #[serde(default = "default_context_padding")]
    pub context_padding: usize,

    /// Maximum number of top files to consider for pivot excerpts.
    #[serde(default = "default_max_pivot_files")]
    pub max_pivot_files: usize,

    /// Minimum remaining budget (in tokens) to continue allocating pivots.
    #[serde(default = "default_min_pivot_budget")]
    pub min_pivot_budget: usize,

    /// Minimum remaining budget (in tokens) to continue allocating bridges.
    #[serde(default = "default_min_bridge_budget")]
    pub min_bridge_budget: usize,

    /// Minimum remaining budget (in tokens) to continue allocating skeletons.
    #[serde(default = "default_min_skeleton_budget")]
    pub min_skeleton_budget: usize,

    /// Maximum number of skeleton files per capsule (limits tree-sitter overhead).
    #[serde(default = "default_max_skeleton_files")]
    pub max_skeleton_files: usize,

    /// Percentage of total token budget reserved for overhead (headers, formatting).
    #[serde(default = "default_overhead_reserve_pct")]
    pub overhead_reserve_pct: usize,

    /// Percentage of usable budget allocated to pivot excerpts.
    #[serde(default = "default_pivot_budget_pct")]
    pub pivot_budget_pct: usize,

    /// Percentage of usable budget allocated to bridge context.
    #[serde(default = "default_bridge_budget_pct")]
    pub bridge_budget_pct: usize,
}

fn default_token_budget() -> usize {
    8000
}
fn default_skeleton_level() -> DetailLevel {
    DetailLevel::Standard
}
fn default_db_path() -> String {
    ".bexp/index.db".to_string()
}
fn default_max_file_size() -> usize {
    1_000_000
}
fn default_watcher_debounce_ms() -> u64 {
    500
}
fn default_memory_budget_pct() -> usize {
    10
}
fn default_session_compress_after_hours() -> u64 {
    2
}
fn default_observation_ttl_days() -> u64 {
    90
}
fn default_reader_pool_size() -> usize {
    4
}
fn default_capsule_cache_size() -> usize {
    100
}
fn default_capsule_cache_ttl_secs() -> u64 {
    300
}
fn default_shutdown_drain_secs() -> u64 {
    5
}
fn default_context_padding() -> usize {
    5
}
fn default_max_pivot_files() -> usize {
    10
}
fn default_min_pivot_budget() -> usize {
    50
}
fn default_min_bridge_budget() -> usize {
    20
}
fn default_min_skeleton_budget() -> usize {
    50
}
fn default_max_skeleton_files() -> usize {
    3
}
fn default_overhead_reserve_pct() -> usize {
    10
}
fn default_pivot_budget_pct() -> usize {
    60
}
fn default_bridge_budget_pct() -> usize {
    10
}

impl Default for BexpConfig {
    fn default() -> Self {
        Self {
            token_budget: default_token_budget(),
            default_skeleton_level: default_skeleton_level(),
            exclude_patterns: default_excludes(),
            db_path: default_db_path(),
            max_file_size: default_max_file_size(),
            watcher_debounce_ms: default_watcher_debounce_ms(),
            memory_budget_pct: default_memory_budget_pct(),
            session_compress_after_hours: default_session_compress_after_hours(),
            observation_ttl_days: default_observation_ttl_days(),
            lsp_resolution: false,
            lsp_servers: HashMap::new(),
            workspace_group: Vec::new(),
            reader_pool_size: default_reader_pool_size(),
            capsule_cache_size: default_capsule_cache_size(),
            capsule_cache_ttl_secs: default_capsule_cache_ttl_secs(),
            health_port: None,
            shutdown_drain_secs: default_shutdown_drain_secs(),
            context_padding: default_context_padding(),
            max_pivot_files: default_max_pivot_files(),
            min_pivot_budget: default_min_pivot_budget(),
            min_bridge_budget: default_min_bridge_budget(),
            min_skeleton_budget: default_min_skeleton_budget(),
            max_skeleton_files: default_max_skeleton_files(),
            overhead_reserve_pct: default_overhead_reserve_pct(),
            pivot_budget_pct: default_pivot_budget_pct(),
            bridge_budget_pct: default_bridge_budget_pct(),
        }
    }
}

fn default_excludes() -> Vec<String> {
    vec![
        "node_modules".into(),
        ".git".into(),
        "target".into(),
        "dist".into(),
        "build".into(),
        "__pycache__".into(),
        ".venv".into(),
        "venv".into(),
        ".next".into(),
        ".nuxt".into(),
        "vendor".into(),
        ".bexp".into(),
    ]
}

impl BexpConfig {
    pub fn load(workspace_root: &Path) -> Result<Self> {
        let config_path = workspace_root.join(".bexp/config.toml");
        let config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| BexpError::Config(format!("Failed to read config: {e}")))?;
            let config: Self = toml::from_str(&content)?;
            config
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.overhead_reserve_pct >= 100 {
            return Err(BexpError::Config(format!(
                "overhead_reserve_pct must be < 100, got {}",
                self.overhead_reserve_pct
            )));
        }
        if self.pivot_budget_pct + self.bridge_budget_pct > 100 {
            return Err(BexpError::Config(format!(
                "pivot_budget_pct ({}) + bridge_budget_pct ({}) must be <= 100",
                self.pivot_budget_pct, self.bridge_budget_pct
            )));
        }
        if self.memory_budget_pct >= 100 {
            return Err(BexpError::Config(format!(
                "memory_budget_pct must be < 100, got {}",
                self.memory_budget_pct
            )));
        }
        Ok(())
    }

    pub fn db_path(&self, workspace_root: &Path) -> PathBuf {
        workspace_root.join(&self.db_path)
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        for component in path.components() {
            let name = component.as_os_str().to_string_lossy();
            for pattern in &self.exclude_patterns {
                if name.as_ref() == pattern.as_str() {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> std::io::Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock went backwards")
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn load_defaults_when_config_file_is_missing() {
        let workspace = TempDir::new("bexp-config-defaults").unwrap();

        let config = BexpConfig::load(workspace.path()).unwrap();

        assert_eq!(config.token_budget, 8000);
        assert_eq!(config.default_skeleton_level, DetailLevel::Standard);
        assert!(config.exclude_patterns.iter().any(|p| p == "node_modules"));
        assert!(config.exclude_patterns.iter().any(|p| p == ".bexp"));
        assert_eq!(config.db_path, ".bexp/index.db");
    }

    #[test]
    fn load_applies_values_from_toml_file() {
        let workspace = TempDir::new("bexp-config-load").unwrap();
        let bexp_dir = workspace.path().join(".bexp");
        fs::create_dir_all(&bexp_dir).unwrap();
        fs::write(
            bexp_dir.join("config.toml"),
            r#"
token_budget = 1234
default_skeleton_level = "minimal"
exclude_patterns = ["generated", "cache"]
db_path = "data/custom.db"
max_file_size = 42
watcher_debounce_ms = 999
memory_budget_pct = 25
session_compress_after_hours = 7
observation_ttl_days = 30
"#,
        )
        .unwrap();

        let config = BexpConfig::load(workspace.path()).unwrap();

        assert_eq!(config.token_budget, 1234);
        assert_eq!(config.default_skeleton_level, DetailLevel::Minimal);
        assert_eq!(
            config.exclude_patterns,
            vec!["generated".to_string(), "cache".to_string()]
        );
        assert_eq!(config.db_path, "data/custom.db");
        assert_eq!(config.max_file_size, 42);
        assert_eq!(config.watcher_debounce_ms, 999);
        assert_eq!(config.memory_budget_pct, 25);
        assert_eq!(config.session_compress_after_hours, 7);
        assert_eq!(config.observation_ttl_days, 30);
    }

    #[test]
    fn is_excluded_checks_path_components() {
        let config = BexpConfig::default();

        assert!(config.is_excluded(Path::new("src/node_modules/pkg/index.ts")));
        assert!(config.is_excluded(Path::new(".bexp/index.db")));
        assert!(!config.is_excluded(Path::new("src/.github/workflows/ci.yml")));
        assert!(!config.is_excluded(Path::new("src/module/git_utils.rs")));
    }
}
