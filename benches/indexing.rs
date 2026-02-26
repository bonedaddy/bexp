use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};

use bexp::config::BexpConfig;
use bexp::db::Database;
use bexp::indexer::IndexerService;
use bexp::skeleton::Skeletonizer;
use bexp::types::DetailLevel;

fn create_workspace(num_files: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bexp_bench_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    for i in 0..num_files {
        let content = format!(
            r#"pub struct Widget{i} {{
    pub id: u64,
    pub name: String,
    pub value: f64,
}}

impl Widget{i} {{
    pub fn new(id: u64, name: String) -> Self {{
        Self {{ id, name, value: 0.0 }}
    }}

    pub fn compute(&self) -> f64 {{
        self.value * self.id as f64
    }}

    pub fn transform(&self, other: &Widget{i}) -> String {{
        format!("{{}}-{{}}", self.name, other.name)
    }}
}}

pub fn process_widget_{i}(w: &Widget{i}) -> u64 {{
    w.id + w.value as u64
}}

pub fn aggregate_{i}(widgets: &[Widget{i}]) -> f64 {{
    widgets.iter().map(|w| w.compute()).sum()
}}
"#
        );
        fs::write(dir.join(format!("mod_{i}.rs")), content).unwrap();
    }

    // Write a lib.rs that references the modules
    let lib_content: String = (0..num_files)
        .map(|i| format!("pub mod mod_{i};"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("lib.rs"), lib_content).unwrap();

    dir
}

fn setup_indexed_workspace(num_files: usize) -> (PathBuf, Arc<Database>, Arc<BexpConfig>) {
    let workspace = create_workspace(num_files);
    let config = Arc::new(BexpConfig::default());
    let db_path = workspace.join(".bexp").join("index.db");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let db = Arc::new(Database::open(&db_path).unwrap());
    (workspace, db, config)
}

fn bench_full_index_10_files(c: &mut Criterion) {
    c.bench_function("full_index_10_files", |b| {
        b.iter_with_setup(
            || setup_indexed_workspace(10),
            |(workspace, db, config)| {
                let indexer = IndexerService::new(db, config, workspace, vec![]);
                indexer.full_index().unwrap();
            },
        );
    });
}

fn bench_full_index_50_files(c: &mut Criterion) {
    c.bench_function("full_index_50_files", |b| {
        b.iter_with_setup(
            || setup_indexed_workspace(50),
            |(workspace, db, config)| {
                let indexer = IndexerService::new(db, config, workspace, vec![]);
                indexer.full_index().unwrap();
            },
        );
    });
}

fn bench_incremental_reindex(c: &mut Criterion) {
    c.bench_function("incremental_reindex", |b| {
        b.iter_with_setup(
            || {
                let (workspace, db, config) = setup_indexed_workspace(20);
                let indexer = IndexerService::new(db, config, workspace.clone(), vec![]);
                indexer.full_index().unwrap();

                // Modify one file
                let changed_file = workspace.join("mod_0.rs");
                let mut content = fs::read_to_string(&changed_file).unwrap();
                content.push_str("\npub fn added_func() -> bool { true }\n");
                fs::write(&changed_file, content).unwrap();

                (indexer, vec![changed_file])
            },
            |(indexer, changed)| {
                indexer.incremental_reindex(&changed).unwrap();
            },
        );
    });
}

fn bench_skeleton_generation(c: &mut Criterion) {
    c.bench_function("skeleton_generation", |b| {
        b.iter_with_setup(
            || {
                let workspace = create_workspace(1);
                let db_path = workspace.join(".bexp").join("index.db");
                fs::create_dir_all(db_path.parent().unwrap()).unwrap();
                let db = Arc::new(Database::open(&db_path).unwrap());

                // Create a medium-sized file to skeletonize
                let medium_file = workspace.join("medium.rs");
                let mut source = String::new();
                for i in 0..20 {
                    source.push_str(&format!(
                        r#"
pub struct Service{i} {{
    db: Arc<Database>,
    count: usize,
}}

impl Service{i} {{
    pub fn new(db: Arc<Database>) -> Self {{
        Self {{ db, count: 0 }}
    }}

    pub fn process(&mut self, input: &str) -> Result<String, Box<dyn std::error::Error>> {{
        self.count += 1;
        let trimmed = input.trim();
        if trimmed.is_empty() {{
            return Err("empty input".into());
        }}
        Ok(format!("processed: {{}}", trimmed))
    }}

    pub fn reset(&mut self) {{
        self.count = 0;
    }}
}}
"#
                    ));
                }
                fs::write(&medium_file, &source).unwrap();

                let skeletonizer = Skeletonizer::new(db);
                (skeletonizer, medium_file)
            },
            |(skeletonizer, file_path)| {
                skeletonizer
                    .skeletonize(&file_path, DetailLevel::Standard)
                    .unwrap();
            },
        );
    });
}

criterion_group!(
    benches,
    bench_full_index_10_files,
    bench_full_index_50_files,
    bench_incremental_reindex,
    bench_skeleton_generation,
);
criterion_main!(benches);
