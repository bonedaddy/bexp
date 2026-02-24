## Vexp Context Engine

This project is indexed by [vexp](https://github.com/user/vexp), a local-first code context engine. Use vexp MCP tools to understand and navigate the codebase efficiently.

### Bootstrap (first time only)

If the project has no `.vexp/` directory yet:
1. Call `mcp__vexp__workspace_setup` to detect the project type and generate config
2. Call `mcp__vexp__trigger_reindex` with `full: true` to build the initial index

### When to use vexp tools

**Understanding code** — before reading files directly:
- `mcp__vexp__get_context_capsule` — primary tool. Describe what you need in natural language and it returns the most relevant files within a token budget. Use this first for any code question.
- `mcp__vexp__query_nodes` — find specific symbols by name, kind (function/struct/trait/etc.), file path, visibility, or export status. Faster than grepping.
- `mcp__vexp__get_skeleton` — get a token-efficient view of a single file (signatures only, bodies collapsed).

**Tracing dependencies** — before refactoring or changing code:
- `mcp__vexp__get_impact_graph` — see callers/callees of a symbol up to N depth. Use `edge_kinds: ["calls"]` to filter. Always check this before modifying a function.
- `mcp__vexp__search_logic_flow` — find execution paths between two symbols.
- `mcp__vexp__query_edges` — query relationships with filters on kind, confidence, direction.

**Inspecting the index**:
- `mcp__vexp__index_status` — file/node/edge counts and language breakdown.
- `mcp__vexp__graph_stats` — graph topology, edge kind breakdown, top symbols by PageRank.
- `mcp__vexp__list_files` — list indexed files with metadata (size, tokens, language).
- `mcp__vexp__get_unresolved_refs` — show cross-file references that couldn't be resolved.
- `mcp__vexp__get_config` — show current vexp configuration.

**Memory across sessions**:
- `mcp__vexp__save_observation` — persist insights, architectural decisions, or gotchas linked to code symbols. These resurface in future capsules.
- `mcp__vexp__search_memory` — search past observations across sessions.
- `mcp__vexp__get_session_context` — retrieve observations from current/previous sessions.
- `mcp__vexp__list_sessions` — list all sessions with observation counts.
- `mcp__vexp__detect_staleness` — mark observations as stale if linked files changed.

**Maintaining the index**:
- `mcp__vexp__trigger_reindex` — call with specific `files` after making changes, or `full: true` after major refactors.
- `mcp__vexp__submit_lsp_edges` — supplement static analysis with type-resolved edges from LSP.

### Rules

1. **Prefer `get_context_capsule` over reading files directly** — it respects token budgets, ranks by relevance, and skeletonizes supporting files.
2. **Check `get_impact_graph` before refactoring** — understand blast radius first.
3. **Use `query_nodes` instead of grep for symbol search** — it's structured and filterable.
4. **Call `trigger_reindex` after making file changes** — keeps the index fresh for subsequent queries.
5. **Use `save_observation` to record non-obvious insights** — they persist across sessions and improve future capsule results.
6. When using `get_context_capsule`, pass `intent` to override auto-detection: `"debug"` for bugs, `"blast_radius"` for impact analysis, `"modify"` for feature work, `"explore"` for general understanding.
