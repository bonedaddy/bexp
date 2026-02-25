## bexp Context Engine

This project is indexed by [bexp](https://github.com/bonedaddy/bexp), a local-first code context engine. Use bexp MCP tools to understand and navigate the codebase efficiently.

### Bootstrap (first time only)

If the project has no `.bexp/` directory yet:
1. Call `mcp__bexp__workspace_setup` to detect the project type and generate config
2. Call `mcp__bexp__trigger_reindex` with `full: true` to build the initial index

### When to use bexp tools

**Understanding code** — before reading files directly:
- `mcp__bexp__get_context_capsule` — primary tool. Describe what you need in natural language and it returns the most relevant files within a token budget. Use this first for any code question.
- `mcp__bexp__query_nodes` — find specific symbols by name, kind (function/struct/trait/etc.), file path, visibility, or export status. Faster than grepping.
- `mcp__bexp__get_skeleton` — get a token-efficient view of a single file (signatures only, bodies collapsed).

**Tracing dependencies** — before refactoring or changing code:
- `mcp__bexp__get_impact_graph` — see callers/callees of a symbol up to N depth. Use `edge_kinds: ["calls"]` to filter. Always check this before modifying a function.
- `mcp__bexp__search_logic_flow` — find execution paths between two symbols.
- `mcp__bexp__query_edges` — query relationships with filters on kind, confidence, direction.

**Inspecting the index**:
- `mcp__bexp__index_status` — file/node/edge counts and language breakdown.
- `mcp__bexp__graph_stats` — graph topology, edge kind breakdown, top symbols by PageRank.
- `mcp__bexp__list_files` — list indexed files with metadata (size, tokens, language).
- `mcp__bexp__get_unresolved_refs` — show cross-file references that couldn't be resolved.
- `mcp__bexp__get_config` — show current bexp configuration.

**Memory across sessions**:
- `mcp__bexp__save_observation` — persist insights, architectural decisions, or gotchas linked to code symbols. These resurface in future capsules.
- `mcp__bexp__search_memory` — search past observations across sessions.
- `mcp__bexp__get_session_context` — retrieve observations from current/previous sessions.
- `mcp__bexp__list_sessions` — list all sessions with observation counts.
- `mcp__bexp__detect_staleness` — mark observations as stale if linked files changed.

**Maintaining the index**:
- `mcp__bexp__trigger_reindex` — call with specific `files` after making changes, or `full: true` after major refactors.
- `mcp__bexp__submit_lsp_edges` — supplement static analysis with type-resolved edges from LSP.

### Rules

1. **Prefer `get_context_capsule` over reading files directly** — it respects token budgets, ranks by relevance, and skeletonizes supporting files.
2. **Check `get_impact_graph` before refactoring** — understand blast radius first.
3. **Use `query_nodes` instead of grep for symbol search** — it's structured and filterable.
4. **Call `trigger_reindex` after making file changes** — keeps the index fresh for subsequent queries.
5. **Use `save_observation` to record non-obvious insights** — they persist across sessions and improve future capsule results.
6. When using `get_context_capsule`, pass `intent` to override auto-detection: `"debug"` for bugs, `"blast_radius"` for impact analysis, `"modify"` for feature work, `"explore"` for general understanding.
