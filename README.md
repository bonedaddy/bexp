# bexp

Local-first context engine for AI coding agents.

bexp indexes source code into a dependency graph of symbols and relationships, then serves token-efficient context over [MCP](https://modelcontextprotocol.io). Instead of dumping entire files into an LLM's context window, bexp returns ranked, skeletonized results within a configurable token budget.

## Features

- **20 MCP tools** — understand, trace, inspect, remember, and maintain a codebase through structured tool calls
- **7 languages** — Rust, TypeScript, JavaScript, Python, C, C++, HTML via tree-sitter grammars
- **Dependency graphs** — cross-file call, import, implements, extends, type_ref, and contains edges powered by petgraph
- **Context capsules** — hybrid search (FTS5 + graph centrality) with intent detection returns full pivot files + skeletonized supporting files within a token budget
- **Hybrid search** — combines BM25 full-text search, PageRank centrality, and recency decay for relevance ranking
- **Cross-session memory** — persist observations linked to code symbols; they resurface automatically in future capsule results
- **File watcher** — automatic incremental reindexing on file changes via notify with configurable debounce
- **Skeletonizer** — collapse function bodies to `{ ... }` while preserving signatures, types, and structure (50–95% token reduction)

## Quick Start

Build from source:

```
cargo build --release
```

Configure your MCP client to use bexp as a server. For Claude Code (`~/.claude/settings.json`):

```json
{
  "mcpServers": {
    "bexp": {
      "command": "/path/to/bexp",
      "args": ["serve", "--workspace", "/path/to/your/project"]
    }
  }
}
```

On first run, call `workspace_setup` to detect the project type and generate config, then `trigger_reindex` with `full: true` to build the initial index.

## Configuration

bexp reads `.bexp/config.toml` from the workspace root. All fields have defaults:

```toml
# Token budget for context capsules (default: 8000)
token_budget = 8000

# Skeleton detail level: "minimal", "standard", or "detailed" (default: "standard")
# minimal = 85-95% reduction, standard = 70-85%, detailed = 50-70%
default_skeleton_level = "standard"

# Database path relative to workspace root (default: ".bexp/index.db")
db_path = ".bexp/index.db"

# Maximum file size in bytes to index (default: 1000000)
max_file_size = 1_000_000

# File watcher debounce in milliseconds (default: 500)
watcher_debounce_ms = 500

# Fraction of token budget reserved for memory/observations (default: 0.10)
memory_budget_pct = 0.10

# Compress session observations after N hours of inactivity (default: 2)
session_compress_after_hours = 2

# Auto-expire observations after N days (default: 90)
observation_ttl_days = 90

# Enable LSP-assisted resolution for dynamic languages (default: false)
lsp_resolution = false

# LSP servers by language
# [lsp_servers.typescript]
# command = "typescript-language-server"
# args = ["--stdio"]

# Directories to exclude from indexing
exclude_patterns = [
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".next",
    ".nuxt",
    "vendor",
    ".bexp",
]
```

Use `workspace_setup` to auto-generate this file based on detected project type.

## MCP Tools

### Understanding Code

| Tool | Description |
|------|-------------|
| `get_context_capsule` | Hybrid search (FTS5 + graph centrality) with intent detection. Returns full pivot files + skeletonized supporting files within a token budget. |
| `get_skeleton` | Token-efficient skeleton of a file. Replaces function bodies with `{ ... }` while preserving signatures. Levels: minimal, standard, detailed. |
| `query_nodes` | Search and filter symbols (functions, classes, structs, etc.) by name, kind, file path, visibility, or export status. |

### Tracing Dependencies

| Tool | Description |
|------|-------------|
| `get_impact_graph` | Impact graph of a symbol showing callers, callees, or dependents up to N depth. Filter by edge kinds. |
| `search_logic_flow` | Find execution paths between two symbols across files. Returns all paths up to max_depth hops. |
| `query_edges` | Query relationships between symbols. Filter by kind (calls/imports/implements/extends/type_ref/contains), confidence, and direction. |

### Inspecting the Index

| Tool | Description |
|------|-------------|
| `index_status` | File/node/edge counts, language breakdown, and watcher state. |
| `graph_stats` | Graph topology statistics: node/edge counts, edge kind breakdown, top-N most central symbols by PageRank. |
| `list_files` | All indexed files with metadata: path, language, size, token count, content hash, and index timestamp. |
| `get_unresolved_refs` | Unresolved cross-file references that couldn't be linked during indexing. Useful for diagnosing graph gaps. |
| `get_config` | Current bexp configuration: token budget, skeleton level, exclude patterns, memory settings. |

### Memory

| Tool | Description |
|------|-------------|
| `save_observation` | Persist an insight or observation linked to code symbols. Auto-generates headline and summary. Resurfaces in future capsules. |
| `search_memory` | Cross-session hybrid search over saved observations. Combines FTS5 BM25 + recency decay (7-day half-life) + graph proximity. |
| `get_session_context` | Retrieve observations from current or previous sessions with staleness flags. |
| `list_sessions` | List all memory sessions with observation counts, timestamps, and summaries. |
| `detect_staleness` | Detect and mark stale observations whose linked files have changed. Cleans up observations past the TTL. |

### Maintenance

| Tool | Description |
|------|-------------|
| `trigger_reindex` | Incremental reindex for specific files, or `full=true` for complete workspace reindex. Rebuilds the graph after indexing. |
| `submit_lsp_edges` | Submit type-resolved call edges from LSP to supplement static analysis. Improves graph accuracy for dynamic languages. |
| `workspace_setup` | Detect project type and generate `.bexp/config.toml` with appropriate settings. |

## Architecture

```
┌─────────────────────────────────────────────┐
│                 MCP Server                  │
│            (JSON-RPC over stdio)            │
├──────────┬──────────┬───────────────────────┤
│ Capsule  │ Skeleton │       Memory          │
│ Engine   │   izer   │      Service          │
├──────────┴──────────┴───────────────────────┤
│        Graph Engine (petgraph)              │
│     PageRank · BFS · Path Search           │
├─────────────────────────────────────────────┤
│      Indexer Service (tree-sitter)          │
│   Rust · TS · JS · Python · C · C++ · HTML │
├─────────────────────────────────────────────┤
│        Database (SQLite + FTS5)             │
│    files · nodes · edges · observations     │
├─────────────────────────────────────────────┤
│          File Watcher (notify)              │
│       debounced incremental reindex         │
└─────────────────────────────────────────────┘
```

## CLI

```
bexp <command> [options]
```

| Command | Description |
|---------|-------------|
| `serve` | Start the MCP server over stdio |
| `index` | Index the workspace |
| `reindex` | Re-index the workspace |
| `flush-wal` | Flush WAL to main database file |

All commands accept `--workspace <path>` (default: current directory).

## Development

```
cargo fmt
cargo clippy
cargo test
cargo audit
```

Set `RUST_LOG` to control log verbosity (logs go to stderr):

```
RUST_LOG=debug cargo run -- serve
```

## License

See [LICENSE](LICENSE) for details.
