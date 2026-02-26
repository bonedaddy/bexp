use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};

use bexp::capsule::search::hybrid_search;
use bexp::capsule::CapsuleGenerator;
use bexp::config::BexpConfig;
use bexp::db::queries::search_nodes_fts;
use bexp::db::Database;
use bexp::graph::GraphEngine;
use bexp::indexer::IndexerService;
use bexp::memory::MemoryService;
use bexp::skeleton::Skeletonizer;
use bexp::types::Intent;

fn create_workspace(num_files: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bexp_bench_search_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    for i in 0..num_files {
        let content = format!(
            r#"pub struct Handler{i} {{
    pub id: u64,
    pub name: String,
    pub active: bool,
}}

impl Handler{i} {{
    pub fn new(id: u64, name: String) -> Self {{
        Self {{ id, name, active: true }}
    }}

    pub fn handle_request(&self, data: &str) -> Result<String, String> {{
        if !self.active {{
            return Err("inactive".to_string());
        }}
        Ok(format!("handler {{}}: {{}}", self.id, data))
    }}

    pub fn deactivate(&mut self) {{
        self.active = false;
    }}
}}

pub fn create_handler_{i}() -> Handler{i} {{
    Handler{i}::new({i}, "handler_{i}".to_string())
}}

pub fn process_request_{i}(handler: &Handler{i}, input: &str) -> String {{
    handler.handle_request(input).unwrap_or_default()
}}
"#
        );
        fs::write(dir.join(format!("handler_{i}.rs")), content).unwrap();
    }

    let lib_content: String = (0..num_files)
        .map(|i| format!("pub mod handler_{i};"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("lib.rs"), lib_content).unwrap();

    dir
}

struct SearchBenchState {
    db: Arc<Database>,
    config: Arc<BexpConfig>,
    graph: Arc<GraphEngine>,
    skeletonizer: Arc<Skeletonizer>,
    memory: Arc<MemoryService>,
}

fn setup_search_workspace() -> SearchBenchState {
    let workspace = create_workspace(30);
    let config = Arc::new(BexpConfig::default());
    let db_path = workspace.join(".bexp").join("index.db");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let db = Arc::new(Database::open(&db_path).unwrap());

    let indexer = IndexerService::new(db.clone(), config.clone(), workspace);
    indexer.full_index().unwrap();

    let graph = Arc::new(GraphEngine::new());
    graph.build_from_db(&db.reader().unwrap()).unwrap();

    let skeletonizer = Arc::new(Skeletonizer::new(db.clone()));
    let memory = Arc::new(MemoryService::new(db.clone(), graph.clone()));

    SearchBenchState {
        db,
        config,
        graph,
        skeletonizer,
        memory,
    }
}

fn bench_hybrid_search(c: &mut Criterion) {
    let state = setup_search_workspace();

    c.bench_function("hybrid_search", |b| {
        b.iter(|| {
            let conn = state.db.reader().unwrap();
            hybrid_search(&conn, &state.graph, "handle_request", &Intent::Explore, 20).unwrap();
        });
    });
}

fn bench_fts5_search(c: &mut Criterion) {
    let state = setup_search_workspace();

    c.bench_function("fts5_search", |b| {
        b.iter(|| {
            let conn = state.db.reader().unwrap();
            search_nodes_fts(&conn, "handler process request", 20).unwrap();
        });
    });
}

fn bench_capsule_generation(c: &mut Criterion) {
    let state = setup_search_workspace();

    let capsule_gen = CapsuleGenerator::new(
        state.db.clone(),
        state.config.clone(),
        state.graph.clone(),
        state.skeletonizer.clone(),
        state.memory.clone(),
    );

    c.bench_function("capsule_generation", |b| {
        b.iter(|| {
            capsule_gen
                .generate("handle_request processing", 8000, None, None)
                .unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_hybrid_search,
    bench_fts5_search,
    bench_capsule_generation,
);
criterion_main!(benches);
