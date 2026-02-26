use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bexp::config::BexpConfig;
use bexp::db::{queries, Database};
use bexp::error::Result;
use bexp::indexer::IndexerService;

struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock went backwards")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_file(&self, relative_path: &str, content: &str) -> std::io::Result<()> {
        let full_path = self.path.join(relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full_path, content)
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn full_index_populates_db_and_resolves_cross_file_call_edges() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-indexer-integration")?;
    workspace.write_file("api.rs", "pub fn helper() {}\n")?;
    workspace.write_file("consumer.rs", "fn run() {\n    helper();\n}\n")?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(db.clone(), config, workspace.path().to_path_buf());

    let report = indexer.full_index()?;
    assert_eq!(report.file_count, 2);
    assert!(report.node_count >= 2);
    assert!(report.edge_count >= 1);

    let reader = db.reader().unwrap();
    let stats = queries::get_index_stats(&reader)?;
    assert_eq!(stats.file_count, 2);
    assert_eq!(stats.node_count as usize, report.node_count);
    assert!(stats.edge_count >= 1);

    let edges = queries::get_all_edges(&reader)?;
    assert!(
        edges.iter().any(|edge| edge.kind == "calls"),
        "expected at least one resolved calls edge, got {:?}",
        edges
            .iter()
            .map(|edge| edge.kind.as_str())
            .collect::<Vec<_>>()
    );

    Ok(())
}
