use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::capsule::CapsuleGenerator;
use crate::config::BexpConfig;
use crate::db::Database;
use crate::graph::GraphEngine;
use crate::indexer::IndexerService;
use crate::memory::MemoryService;
use crate::skeleton::Skeletonizer;

use super::server::BexpServer;

pub struct TempWorkspace {
    pub path: PathBuf,
}

impl TempWorkspace {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock went backwards")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("failed to create temp workspace");
        Self { path }
    }

    fn write_file(&self, relative_path: &str, content: &str) {
        let full_path = self.path.join(relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(full_path, content).expect("failed to write file");
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub struct TestServerBuilder {
    files: Vec<(String, String)>,
}

impl TestServerBuilder {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn with_file(mut self, path: &str, content: &str) -> Self {
        self.files.push((path.to_string(), content.to_string()));
        self
    }

    pub fn build(self) -> (BexpServer, TempWorkspace) {
        let workspace = TempWorkspace::new("bexp-mcp-test");

        for (path, content) in &self.files {
            workspace.write_file(path, content);
        }

        let config = Arc::new(BexpConfig::default());
        let db = Arc::new(Database::open_test().expect("failed to open test db"));
        let graph = Arc::new(GraphEngine::new());
        let indexer = Arc::new(IndexerService::new(
            db.clone(),
            config.clone(),
            workspace.path().to_path_buf(),
        ));

        // Index files if any were provided
        if !self.files.is_empty() {
            indexer.full_index().expect("failed to full_index");
            graph
                .build_from_db(&db.reader())
                .expect("failed to build graph");
        }
        indexer.set_index_ready(true);

        let skeletonizer = Arc::new(Skeletonizer::new(db.clone()));
        let memory = Arc::new(MemoryService::new(db.clone(), graph.clone()));
        let capsule = Arc::new(CapsuleGenerator::new(
            db.clone(),
            config.clone(),
            graph.clone(),
            skeletonizer.clone(),
            memory.clone(),
        ));

        let server = BexpServer::new(
            db,
            config,
            indexer,
            graph,
            skeletonizer,
            capsule,
            memory,
            workspace.path().to_path_buf(),
        );

        (server, workspace)
    }
}
