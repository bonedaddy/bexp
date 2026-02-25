use rusqlite_migration::{Migrations, M};

/// Migration 1: Baseline schema (original).
const M1_BASELINE: &str = r#"
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    language TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    mtime_ns INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    token_count INTEGER,
    skeleton_minimal TEXT,
    skeleton_standard TEXT,
    skeleton_detailed TEXT,
    skeleton_minimal_tokens INTEGER,
    skeleton_standard_tokens INTEGER,
    skeleton_detailed_tokens INTEGER,
    indexed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);
CREATE INDEX IF NOT EXISTS idx_files_content_hash ON files(content_hash);

CREATE TABLE IF NOT EXISTS nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT,
    signature TEXT,
    docstring TEXT,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    col_start INTEGER NOT NULL DEFAULT 0,
    col_end INTEGER NOT NULL DEFAULT 0,
    visibility TEXT,
    is_export INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_nodes_file_id ON nodes(file_id);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_qualified_name ON nodes(qualified_name);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);

CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 1.0
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_node_id);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_node_id);
CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);

CREATE TABLE IF NOT EXISTS unresolved_refs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_name TEXT NOT NULL,
    target_qualified_name TEXT,
    edge_kind TEXT NOT NULL,
    import_path TEXT
);

CREATE INDEX IF NOT EXISTS idx_unresolved_source ON unresolved_refs(source_node_id);
CREATE INDEX IF NOT EXISTS idx_unresolved_target ON unresolved_refs(target_name);

CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
    name,
    qualified_name,
    signature,
    docstring,
    content='nodes',
    content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS nodes_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(rowid, name, qualified_name, signature, docstring)
    VALUES (new.id, new.name, new.qualified_name, new.signature, new.docstring);
END;

CREATE TRIGGER IF NOT EXISTS nodes_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, signature, docstring)
    VALUES ('delete', old.id, old.name, old.qualified_name, old.signature, old.docstring);
END;

CREATE TRIGGER IF NOT EXISTS nodes_au AFTER UPDATE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, signature, docstring)
    VALUES ('delete', old.id, old.name, old.qualified_name, old.signature, old.docstring);
    INSERT INTO nodes_fts(rowid, name, qualified_name, signature, docstring)
    VALUES (new.id, new.name, new.qualified_name, new.signature, new.docstring);
END;

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    compressed INTEGER NOT NULL DEFAULT 0,
    summary TEXT
);

CREATE TABLE IF NOT EXISTS observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    headline TEXT,
    summary TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    is_stale INTEGER NOT NULL DEFAULT 0,
    stale_reason TEXT
);

CREATE INDEX IF NOT EXISTS idx_observations_session ON observations(session_id);
CREATE INDEX IF NOT EXISTS idx_observations_created ON observations(created_at);

CREATE TABLE IF NOT EXISTS observation_symbols (
    observation_id INTEGER NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    PRIMARY KEY (observation_id, node_id)
);

CREATE TABLE IF NOT EXISTS observation_files (
    observation_id INTEGER NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    content_hash_at_link TEXT NOT NULL,
    PRIMARY KEY (observation_id, file_id)
);

CREATE VIRTUAL TABLE IF NOT EXISTS observations_fts USING fts5(
    content,
    headline,
    summary,
    content='observations',
    content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS obs_ai AFTER INSERT ON observations BEGIN
    INSERT INTO observations_fts(rowid, content, headline, summary)
    VALUES (new.id, new.content, new.headline, new.summary);
END;

CREATE TRIGGER IF NOT EXISTS obs_ad AFTER DELETE ON observations BEGIN
    INSERT INTO observations_fts(observations_fts, rowid, content, headline, summary)
    VALUES ('delete', old.id, old.content, old.headline, old.summary);
END;

CREATE TRIGGER IF NOT EXISTS obs_au AFTER UPDATE ON observations BEGIN
    INSERT INTO observations_fts(observations_fts, rowid, content, headline, summary)
    VALUES ('delete', old.id, old.content, old.headline, old.summary);
    INSERT INTO observations_fts(rowid, content, headline, summary)
    VALUES (new.id, new.content, new.headline, new.summary);
END;

CREATE TABLE IF NOT EXISTS index_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Migration 2: Add tokenized name columns for FTS5, metadata column on nodes.
/// Drop + recreate FTS5 table and triggers to include tokenized columns.
const M2_TOKENIZED_FTS: &str = r#"
ALTER TABLE nodes ADD COLUMN tokenized_name TEXT;
ALTER TABLE nodes ADD COLUMN tokenized_qualified_name TEXT;
ALTER TABLE nodes ADD COLUMN metadata TEXT;

-- Drop old FTS triggers and table, recreate with tokenized columns
DROP TRIGGER IF EXISTS nodes_ai;
DROP TRIGGER IF EXISTS nodes_ad;
DROP TRIGGER IF EXISTS nodes_au;
DROP TABLE IF EXISTS nodes_fts;

CREATE VIRTUAL TABLE nodes_fts USING fts5(
    name,
    qualified_name,
    tokenized_name,
    tokenized_qualified_name,
    signature,
    docstring,
    content='nodes',
    content_rowid='id'
);

CREATE TRIGGER nodes_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(rowid, name, qualified_name, tokenized_name, tokenized_qualified_name, signature, docstring)
    VALUES (new.id, new.name, new.qualified_name, new.tokenized_name, new.tokenized_qualified_name, new.signature, new.docstring);
END;

CREATE TRIGGER nodes_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, tokenized_name, tokenized_qualified_name, signature, docstring)
    VALUES ('delete', old.id, old.name, old.qualified_name, old.tokenized_name, old.tokenized_qualified_name, old.signature, old.docstring);
END;

CREATE TRIGGER nodes_au AFTER UPDATE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, tokenized_name, tokenized_qualified_name, signature, docstring)
    VALUES ('delete', old.id, old.name, old.qualified_name, old.tokenized_name, old.tokenized_qualified_name, old.signature, old.docstring);
    INSERT INTO nodes_fts(rowid, name, qualified_name, tokenized_name, tokenized_qualified_name, signature, docstring)
    VALUES (new.id, new.name, new.qualified_name, new.tokenized_name, new.tokenized_qualified_name, new.signature, new.docstring);
END;
"#;

/// Migration 3: Add context column to edges and unresolved_refs for control flow extraction.
const M3_CONTROL_FLOW: &str = r#"
ALTER TABLE edges ADD COLUMN context TEXT;
ALTER TABLE unresolved_refs ADD COLUMN context TEXT;
"#;

/// Migration 4: Cross-workspace edges table.
const M4_CROSS_WORKSPACE: &str = r#"
CREATE TABLE IF NOT EXISTS cross_workspace_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_workspace TEXT NOT NULL,
    target_qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.8
);

CREATE INDEX IF NOT EXISTS idx_cross_ws_source ON cross_workspace_edges(source_node_id);
CREATE INDEX IF NOT EXISTS idx_cross_ws_target ON cross_workspace_edges(target_qualified_name);
"#;

/// Migration 5: Indexes to speed up scope-aware resolution and node range queries.
const M5_RESOLVER_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_nodes_file_id_name ON nodes(file_id, name);
CREATE INDEX IF NOT EXISTS idx_unresolved_import_path ON unresolved_refs(import_path);
CREATE INDEX IF NOT EXISTS idx_edges_source_kind ON edges(source_node_id, kind);
CREATE INDEX IF NOT EXISTS idx_nodes_file_line_range ON nodes(file_id, line_start, line_end);
"#;

pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(M1_BASELINE),
        M::up(M2_TOKENIZED_FTS),
        M::up(M3_CONTROL_FLOW),
        M::up(M4_CROSS_WORKSPACE),
        M::up(M5_RESOLVER_INDEXES),
    ])
}
