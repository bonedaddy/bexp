## bexp Context Engine

This project is indexed by [bexp](https://github.com/bonedaddy/bexp), a local-first code context engine. Use bexp MCP tools to understand and navigate the codebase efficiently.

### Bootstrap (first time only)

If the project has no `.bexp/` directory yet:
1. Call `mcp__bexp__workspace_setup` to detect the project type and generate config
2. Call `mcp__bexp__trigger_reindex` with `full: true` to build the initial index

### When to use bexp tools

**Search/understand code** (use these first — before reading files directly):
- `mcp__bexp__get_context_capsule` — **primary code search tool**. Describe what you need in natural language and it returns the most relevant files within a token budget, with intent detection, pivot excerpts, bridge signatures, and skeletonized supporting files. Use this first for any code question.
- `mcp__bexp__query_nodes` — search indexed code symbols (functions, structs, traits, etc.) by name, kind, file path, visibility, or export status. Returns structured results with signatures and locations. Use for targeted symbol lookup.
- `mcp__bexp__get_skeleton` — get a token-efficient view of a single file (signatures only, bodies collapsed). Levels: minimal (85-95% reduction), standard (70-85%), detailed (50-70%).

**Tracing dependencies** — before refactoring or changing code:
- `mcp__bexp__get_impact_graph` — see callers/callees of a symbol up to N depth. Use `edge_kinds: ["calls"]` to filter. Always check this before modifying a function.
- `mcp__bexp__search_logic_flow` — find execution paths between two symbols across files.
- `mcp__bexp__query_edges` — query relationships with filters on kind (calls/imports/implements/extends/type_ref/contains), confidence, and direction.

**Inspecting the index**:
- `mcp__bexp__index_status` — file/node/edge counts, language breakdown, and watcher state.
- `mcp__bexp__graph_stats` — graph topology, edge kind breakdown, top-N most central symbols by PageRank.
- `mcp__bexp__list_files` — list indexed files with metadata (path, language, size, token count, content hash, index timestamp).
- `mcp__bexp__get_unresolved_refs` — show cross-file references that couldn't be resolved. Useful for diagnosing graph gaps.
- `mcp__bexp__get_config` — show current bexp configuration (token budget, skeleton level, exclude patterns, memory settings).

**Session memory** (NOT for code search — only searches previously saved observations):
- `mcp__bexp__save_observation` — persist insights, architectural decisions, or gotchas linked to code symbols. These resurface in future capsules.
- `mcp__bexp__search_memory` — search previously saved observations/insights only. Does NOT search code. For code search use `query_nodes` or `get_context_capsule`.
- `mcp__bexp__get_session_context` — retrieve observations from current/previous sessions with staleness flags.
- `mcp__bexp__list_sessions` — list all sessions with observation counts, timestamps, and summaries.
- `mcp__bexp__detect_staleness` — detect and mark stale observations whose linked files have changed. Also cleans up observations past the TTL.

**Maintaining the index**:
- `mcp__bexp__trigger_reindex` — call with specific `files` after making changes, or `full: true` after major refactors. Rebuilds the graph after indexing.
- `mcp__bexp__submit_lsp_edges` — supplement static analysis with type-resolved call edges from LSP. Improves graph accuracy for dynamic languages.

### Rules

1. **Prefer `get_context_capsule` over reading files directly** — it respects token budgets, ranks by relevance, and skeletonizes supporting files.
2. **Check `get_impact_graph` before refactoring** — understand blast radius first.
3. **Use `query_nodes` instead of grep for symbol search** — it's structured and filterable.
4. **Do NOT use `search_memory` to find code** — it only searches saved observations. Use `get_context_capsule` or `query_nodes` for code search.
5. **Call `trigger_reindex` after making file changes** — keeps the index fresh for subsequent queries.
6. **Use `save_observation` to record non-obvious insights** — they persist across sessions and improve future capsule results.
7. When using `get_context_capsule`, pass `intent` to override auto-detection: `"debug"` for bugs, `"blast_radius"` for impact analysis, `"modify"` for feature work, `"explore"` for general understanding.
