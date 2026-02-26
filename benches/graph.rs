use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};

use bexp::config::BexpConfig;
use bexp::db::Database;
use bexp::graph::GraphEngine;
use bexp::indexer::IndexerService;

fn create_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bexp_bench_graph_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    // Create files with inter-file references to produce a richer graph
    for i in 0..30 {
        let mut content = format!(
            r#"pub struct Node{i} {{
    pub id: u64,
    pub label: String,
}}

impl Node{i} {{
    pub fn new(id: u64) -> Self {{
        Self {{ id, label: format!("node_{{}}", id) }}
    }}

    pub fn connect(&self, target_id: u64) -> String {{
        format!("{{}} -> {{}}", self.id, target_id)
    }}
}}
"#,
        );

        // Add cross-references to prior modules
        if i > 0 {
            content.push_str(&format!(
                r#"
pub fn link_{i}_to_{prev}(a: &Node{i}, _b: &Node{prev}) -> String {{
    a.connect({prev})
}}
"#,
                i = i,
                prev = i - 1,
            ));
        }
        if i > 1 {
            content.push_str(&format!(
                r#"
pub fn bridge_{i}_to_{prev2}(a: &Node{i}, _b: &Node{prev2}) -> String {{
    a.connect({prev2})
}}
"#,
                i = i,
                prev2 = i - 2,
            ));
        }

        fs::write(dir.join(format!("node_{i}.rs")), content).unwrap();
    }

    let lib_content: String = (0..30)
        .map(|i| format!("pub mod node_{i};"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("lib.rs"), lib_content).unwrap();

    dir
}

struct GraphBenchState {
    db: Arc<Database>,
}

fn setup_graph_workspace() -> GraphBenchState {
    let workspace = create_workspace();
    let config = Arc::new(BexpConfig::default());
    let db_path = workspace.join(".bexp").join("index.db");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let db = Arc::new(Database::open(&db_path).unwrap());

    let indexer = IndexerService::new(db.clone(), config, workspace);
    indexer.full_index().unwrap();

    GraphBenchState { db }
}

fn bench_graph_build(c: &mut Criterion) {
    let state = setup_graph_workspace();

    c.bench_function("graph_build", |b| {
        b.iter(|| {
            let graph = GraphEngine::new();
            graph.build_from_db(&state.db.reader()).unwrap();
        });
    });
}

fn bench_pagerank(c: &mut Criterion) {
    let state = setup_graph_workspace();

    c.bench_function("pagerank", |b| {
        b.iter(|| {
            let graph = GraphEngine::new();
            graph.rebuild_from_db(&state.db.reader()).unwrap();
        });
    });
}

fn bench_impact_graph(c: &mut Criterion) {
    let state = setup_graph_workspace();

    // Build the graph once; then benchmark impact_graph queries
    let graph = GraphEngine::new();
    graph.build_from_db(&state.db.reader()).unwrap();

    c.bench_function("impact_graph", |b| {
        b.iter(|| {
            // Use a known symbol pattern from our generated files
            let _ = graph.impact_graph("Node0", "both", 3, None);
        });
    });
}

fn bench_find_paths(c: &mut Criterion) {
    let state = setup_graph_workspace();

    let graph = GraphEngine::new();
    graph.build_from_db(&state.db.reader()).unwrap();

    c.bench_function("find_paths", |b| {
        b.iter(|| {
            let _ = graph.find_paths("Node0", "Node29", 5);
        });
    });
}

criterion_group!(
    benches,
    bench_graph_build,
    bench_pagerank,
    bench_impact_graph,
    bench_find_paths,
);
criterion_main!(benches);
