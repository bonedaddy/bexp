use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bexp::capsule::CapsuleGenerator;
use bexp::config::BexpConfig;
use bexp::db::{queries, Database};
use bexp::error::Result;
use bexp::graph::GraphEngine;
use bexp::indexer::IndexerService;
use bexp::memory::MemoryService;
use bexp::skeleton::Skeletonizer;
use bexp::types::DetailLevel;

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
fn full_index_then_graph_has_correct_topology() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-pipeline-graph")?;
    workspace.write_file("lib.rs", "pub fn alpha() {\n    beta();\n}\n")?;
    workspace.write_file("helper.rs", "pub fn beta() {}\n")?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(db.clone(), config, workspace.path().to_path_buf(), vec![]);

    let report = indexer.full_index()?;
    assert_eq!(report.file_count, 2);

    let graph = GraphEngine::new();
    graph.build_from_db(&db.reader().unwrap())?;

    assert!(
        graph.node_count() >= 2,
        "expected at least 2 nodes, got {}",
        graph.node_count()
    );
    assert!(
        graph.edge_count() >= 1,
        "expected at least 1 edge, got {}",
        graph.edge_count()
    );

    Ok(())
}

#[test]
fn capsule_generation_returns_relevant_content() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-pipeline-capsule")?;
    workspace.write_file(
        "api.rs",
        "pub struct ApiClient;\n\
         impl ApiClient {\n\
         \x20   pub fn fetch(&self) -> String {\n\
         \x20       String::new()\n\
         \x20   }\n\
         }\n",
    )?;
    workspace.write_file(
        "handler.rs",
        "fn handle() {\n    let _client = ApiClient::new();\n}\n",
    )?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(
        db.clone(),
        config.clone(),
        workspace.path().to_path_buf(),
        vec![],
    );

    indexer.full_index()?;

    let graph = Arc::new(GraphEngine::new());
    graph.build_from_db(&db.reader().unwrap())?;

    let skeletonizer = Arc::new(Skeletonizer::new(db.clone()));
    let memory = Arc::new(MemoryService::new(db.clone(), graph.clone()));
    let capsule = CapsuleGenerator::new(db.clone(), config, graph, skeletonizer, memory);

    let output = capsule.generate("ApiClient fetch", 4000, None, None)?;
    assert!(!output.is_empty(), "capsule output should not be empty");

    Ok(())
}

#[test]
fn incremental_reindex_updates_graph() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-pipeline-incremental")?;
    workspace.write_file("lib.rs", "pub fn original() {}\n")?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(db.clone(), config, workspace.path().to_path_buf(), vec![]);

    indexer.full_index()?;

    let graph = GraphEngine::new();
    graph.build_from_db(&db.reader().unwrap())?;
    let initial_node_count = graph.node_count();

    // Add a new file that calls the original function
    std::thread::sleep(std::time::Duration::from_millis(50));
    workspace.write_file("extra.rs", "fn use_it() {\n    original();\n}\n")?;

    let extra_path = workspace.path().join("extra.rs");
    indexer.incremental_reindex(&[extra_path])?;

    graph.rebuild_from_db(&db.reader().unwrap())?;

    assert!(
        graph.node_count() > initial_node_count,
        "node count should grow after adding a file: {} vs {}",
        graph.node_count(),
        initial_node_count
    );

    Ok(())
}

#[test]
fn skeleton_generation_after_indexing() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-pipeline-skeleton")?;
    let source = "\
pub struct Widget {
    name: String,
    value: i32,
}

impl Widget {
    pub fn new(name: &str, value: i32) -> Self {
        Self {
            name: name.to_string(),
            value,
        }
    }

    pub fn display(&self) -> String {
        format!(\"{}: {}\", self.name, self.value)
    }
}
";
    workspace.write_file("module.rs", source)?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(db.clone(), config, workspace.path().to_path_buf(), vec![]);

    indexer.full_index()?;

    let skeletonizer = Skeletonizer::new(db);
    let skeleton =
        skeletonizer.skeletonize(&workspace.path().join("module.rs"), DetailLevel::Standard)?;

    assert!(
        skeleton.contains("Widget"),
        "skeleton should contain struct name 'Widget', got: {skeleton}"
    );
    assert!(
        skeleton.contains("new"),
        "skeleton should contain method name 'new', got: {skeleton}"
    );
    assert!(
        skeleton.contains("display"),
        "skeleton should contain method name 'display', got: {skeleton}"
    );
    assert!(
        skeleton.len() < source.len(),
        "skeleton ({} bytes) should be shorter than source ({} bytes)",
        skeleton.len(),
        source.len()
    );

    Ok(())
}

#[test]
fn memory_observations_persist_across_sessions() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-pipeline-memory")?;
    workspace.write_file("lib.rs", "pub fn target_func() {}\n")?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(db.clone(), config, workspace.path().to_path_buf(), vec![]);

    indexer.full_index()?;

    let graph = Arc::new(GraphEngine::new());
    graph.build_from_db(&db.reader().unwrap())?;

    let memory = MemoryService::new(db.clone(), graph);

    let session_id = "test-session-1";
    let symbols = vec!["target_func".to_string()];
    let save_result = memory.save_observation(
        session_id,
        "target_func has a performance issue when called with large inputs",
        Some(&symbols),
        None,
    )?;
    assert!(
        !save_result.is_empty(),
        "save_observation should return confirmation"
    );

    let search_result = memory.search("target_func performance", 5, Some(session_id))?;
    assert!(
        !search_result.is_empty(),
        "search should find the saved observation"
    );

    let context = memory.get_session_context(Some(session_id), false, 0)?;
    assert!(
        context.contains("target_func"),
        "session context should contain the observation content"
    );

    Ok(())
}

#[test]
fn multi_language_indexing() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-pipeline-multilang")?;
    workspace.write_file("lib.rs", "pub fn rust_func() {}\n")?;
    workspace.write_file("module.py", "def python_func():\n    pass\n")?;
    workspace.write_file("index.ts", "export function tsFunc(): void {}\n")?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(db.clone(), config, workspace.path().to_path_buf(), vec![]);

    let report = indexer.full_index()?;
    assert_eq!(report.file_count, 3, "should index exactly 3 files");

    let reader = db.reader().unwrap();
    let stats = queries::get_index_stats(&reader)?;
    assert_eq!(stats.file_count, 3);

    let languages: Vec<&str> = stats
        .language_breakdown
        .iter()
        .map(|(l, _)| l.as_str())
        .collect();
    assert!(
        languages.contains(&"rust"),
        "should contain rust, got {languages:?}"
    );
    assert!(
        languages.contains(&"python"),
        "should contain python, got {languages:?}"
    );
    assert!(
        languages.contains(&"typescript"),
        "should contain typescript, got {languages:?}"
    );

    Ok(())
}

#[test]
fn cross_file_references_create_call_edges() -> Result<()> {
    let workspace = TempWorkspace::new("bexp-pipeline-crossref")?;
    workspace.write_file(
        "provider.rs",
        "pub fn provide_data() -> i32 { 42 }\npub fn transform(x: i32) -> i32 { x * 2 }\n",
    )?;
    workspace.write_file(
        "consumer.rs",
        "fn consume() {\n    let data = provide_data();\n    let _result = transform(data);\n}\n",
    )?;

    let config = Arc::new(BexpConfig::default());
    let db = Arc::new(Database::open(&config.db_path(workspace.path()))?);
    let indexer = IndexerService::new(db.clone(), config, workspace.path().to_path_buf(), vec![]);

    indexer.full_index()?;

    let reader = db.reader().unwrap();
    let edges = queries::get_all_edges(&reader)?;
    assert!(
        edges.iter().any(|e| e.kind == "calls"),
        "expected at least one 'calls' edge, got kinds: {:?}",
        edges.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>()
    );

    Ok(())
}
