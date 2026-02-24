use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;

use crate::capsule::CapsuleGenerator;
use crate::config::VexpConfig;
use crate::db::Database;
use crate::graph::GraphEngine;
use crate::indexer::IndexerService;
use crate::memory::MemoryService;
use crate::skeleton::Skeletonizer;

pub struct VexpServer {
    pub db: Arc<Database>,
    pub config: Arc<VexpConfig>,
    pub indexer: Arc<IndexerService>,
    pub graph: Arc<GraphEngine>,
    pub skeletonizer: Arc<Skeletonizer>,
    pub capsule: Arc<CapsuleGenerator>,
    pub memory: Arc<MemoryService>,
    pub workspace_root: std::path::PathBuf,
    tool_router: ToolRouter<Self>,
}

// -- Tool parameter types --

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CapsuleParams {
    #[schemars(description = "Natural language query describing what context you need")]
    pub query: String,
    #[schemars(description = "Maximum token budget (default: from config)")]
    pub token_budget: Option<usize>,
    #[schemars(description = "Session ID for memory integration")]
    pub session_id: Option<String>,
    #[schemars(description = "Override auto-detected intent: 'debug', 'blast_radius', 'modify', 'explore'")]
    pub intent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ImpactParams {
    #[schemars(description = "Symbol name to analyze")]
    pub symbol: String,
    #[schemars(description = "Direction: 'callers', 'callees', or 'both'")]
    pub direction: Option<String>,
    #[schemars(description = "Maximum traversal depth (default: 3)")]
    pub depth: Option<usize>,
    #[schemars(description = "Filter edge kinds to traverse, e.g. ['calls', 'imports']")]
    pub edge_kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FlowParams {
    #[schemars(description = "Source symbol name")]
    pub from_symbol: String,
    #[schemars(description = "Target symbol name")]
    pub to_symbol: String,
    #[schemars(description = "Maximum path length (default: 5)")]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SkeletonParams {
    #[schemars(description = "File path relative to workspace root")]
    pub file_path: String,
    #[schemars(description = "Detail level: 'minimal', 'standard', or 'detailed'")]
    pub level: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetupParams {
    #[schemars(description = "Force regeneration of config files")]
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LspEdgesParams {
    #[schemars(description = "Array of edges with source, target, and kind")]
    pub edges: Vec<LspEdge>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LspEdge {
    pub source_qualified_name: String,
    pub target_qualified_name: String,
    pub kind: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionParams {
    #[schemars(description = "Session ID (omit to use current session)")]
    pub session_id: Option<String>,
    #[schemars(description = "Include observations from previous sessions")]
    pub include_previous: Option<bool>,
    #[schemars(description = "Number of previous sessions to include (default: 3)")]
    pub previous_limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemorySearchParams {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Maximum results to return (default: 10)")]
    pub limit: Option<usize>,
    #[schemars(description = "Session ID for proximity scoring")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ObserveParams {
    #[schemars(description = "The observation content")]
    pub content: String,
    #[schemars(description = "Session ID")]
    pub session_id: String,
    #[schemars(description = "Symbol names this observation relates to")]
    pub symbols: Option<Vec<String>>,
    #[schemars(description = "File paths this observation relates to")]
    pub files: Option<Vec<String>>,
}

// -- New tool parameter types --

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryNodesParams {
    #[schemars(description = "Search term to match against symbol names")]
    pub query: Option<String>,
    #[schemars(description = "Filter by node kind: function, method, class, struct, interface, enum, trait, impl, module, variable, constant, import")]
    pub kind: Option<String>,
    #[schemars(description = "Filter by file path (substring match)")]
    pub file_path: Option<String>,
    #[schemars(description = "Filter by visibility: public, private, protected")]
    pub visibility: Option<String>,
    #[schemars(description = "Only return exported symbols")]
    pub exported_only: Option<bool>,
    #[schemars(description = "Include PageRank centrality scores")]
    pub include_pagerank: Option<bool>,
    #[schemars(description = "Maximum results (default: 50)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryEdgesParams {
    #[schemars(description = "Symbol name to filter edges by")]
    pub symbol: Option<String>,
    #[schemars(description = "Filter by edge kind: calls, imports, implements, extends, type_ref, contains")]
    pub kind: Option<String>,
    #[schemars(description = "Minimum confidence threshold (0.0 - 1.0)")]
    pub min_confidence: Option<f64>,
    #[schemars(description = "Direction relative to symbol: 'incoming', 'outgoing', or 'both'")]
    pub direction: Option<String>,
    #[schemars(description = "Maximum results (default: 50)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GraphStatsParams {
    #[schemars(description = "Number of top nodes by PageRank to return (default: 20)")]
    pub top_n: Option<usize>,
    #[schemars(description = "Filter top-N by node kind")]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListFilesParams {
    #[schemars(description = "Filter by language: typescript, javascript, python, rust, html, c, cpp")]
    pub language: Option<String>,
    #[schemars(description = "Sort by: 'path' (default), 'size', 'tokens', 'indexed_at'")]
    pub sort_by: Option<String>,
    #[schemars(description = "Maximum results (default: 100)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UnresolvedRefsParams {
    #[schemars(description = "Filter by file path (substring match)")]
    pub file_path: Option<String>,
    #[schemars(description = "Maximum results (default: 50)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListSessionsParams {
    #[schemars(description = "Maximum results (default: 20)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReindexParams {
    #[schemars(description = "Specific file paths to reindex (relative to workspace root)")]
    pub files: Option<Vec<String>>,
    #[schemars(description = "Perform a full reindex of the entire workspace")]
    pub full: Option<bool>,
}

#[tool_router]
impl VexpServer {
    pub fn new(
        db: Arc<Database>,
        config: Arc<VexpConfig>,
        indexer: Arc<IndexerService>,
        graph: Arc<GraphEngine>,
        skeletonizer: Arc<Skeletonizer>,
        capsule: Arc<CapsuleGenerator>,
        memory: Arc<MemoryService>,
        workspace_root: std::path::PathBuf,
    ) -> Self {
        Self {
            db,
            config,
            indexer,
            graph,
            skeletonizer,
            capsule,
            memory,
            workspace_root,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Get a token-efficient context capsule for a query. Uses hybrid search (FTS5 + graph centrality) with intent detection to return full pivot files + skeletonized supporting files within a token budget.")]
    async fn get_context_capsule(
        &self,
        Parameters(params): Parameters<CapsuleParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::capsule::handle(self, params).await
    }

    #[tool(description = "Get the impact graph of a symbol showing callers, callees, or dependents up to N depth. Useful for understanding blast radius of changes. Optionally filter by edge kinds.")]
    async fn get_impact_graph(
        &self,
        Parameters(params): Parameters<ImpactParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::impact::handle(self, params).await
    }

    #[tool(description = "Find execution paths between two symbols across files. Returns all paths up to max_depth hops.")]
    async fn search_logic_flow(
        &self,
        Parameters(params): Parameters<FlowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::flow::handle(self, params).await
    }

    #[tool(description = "Get a token-efficient skeleton of a file. Replaces function/method bodies with { ... } while preserving signatures, types, and structure. Levels: minimal (85-95% reduction), standard (70-85%), detailed (50-70%).")]
    async fn get_skeleton(
        &self,
        Parameters(params): Parameters<SkeletonParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::skeleton::handle(self, params).await
    }

    #[tool(description = "Get the current index status: file/node/edge counts, language breakdown, and watcher state.")]
    async fn index_status(&self) -> Result<CallToolResult, ErrorData> {
        super::tools::status::handle(self).await
    }

    #[tool(description = "Detect project type and generate .vexp/config.toml with appropriate settings.")]
    async fn workspace_setup(
        &self,
        Parameters(params): Parameters<SetupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::setup::handle(self, params).await
    }

    #[tool(description = "Submit type-resolved call edges from LSP to supplement static analysis. Improves graph accuracy for dynamic languages.")]
    async fn submit_lsp_edges(
        &self,
        Parameters(params): Parameters<LspEdgesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::lsp::handle(self, params).await
    }

    #[tool(description = "Retrieve observations from current or previous sessions with staleness flags. Returns insights linked to code symbols.")]
    async fn get_session_context(
        &self,
        Parameters(params): Parameters<SessionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::session::handle(self, params).await
    }

    #[tool(description = "Cross-session hybrid search over saved observations. Combines FTS5 BM25 + recency decay (7-day half-life) + graph proximity.")]
    async fn search_memory(
        &self,
        Parameters(params): Parameters<MemorySearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::memory_search::handle(self, params).await
    }

    #[tool(description = "Persist an insight or observation linked to code symbols. Auto-generates headline and summary. Observations persist across sessions and are surfaced in context capsules.")]
    async fn save_observation(
        &self,
        Parameters(params): Parameters<ObserveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::observe::handle(self, params).await
    }

    // -- New tools --

    #[tool(description = "Search and filter code symbols (functions, classes, structs, etc.) with structured queries. Filter by name, kind, file path, visibility, or export status.")]
    async fn query_nodes(
        &self,
        Parameters(params): Parameters<QueryNodesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::query_nodes::handle(self, params).await
    }

    #[tool(description = "Query relationships between symbols with filters. Filter by symbol, edge kind (calls/imports/implements/extends/type_ref/contains), confidence, and direction.")]
    async fn query_edges(
        &self,
        Parameters(params): Parameters<QueryEdgesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::query_edges::handle(self, params).await
    }

    #[tool(description = "Get graph topology statistics: node/edge counts, edge kind breakdown, and top-N most central symbols by PageRank.")]
    async fn graph_stats(
        &self,
        Parameters(params): Parameters<GraphStatsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::graph_stats::handle(self, params).await
    }

    #[tool(description = "List all indexed files with metadata: path, language, size, token count, content hash, and index timestamp.")]
    async fn list_files(
        &self,
        Parameters(params): Parameters<ListFilesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::list_files::handle(self, params).await
    }

    #[tool(description = "Show unresolved cross-file references that couldn't be linked during indexing. Useful for diagnosing graph gaps.")]
    async fn get_unresolved_refs(
        &self,
        Parameters(params): Parameters<UnresolvedRefsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::unresolved::handle(self, params).await
    }

    #[tool(description = "List all memory sessions with observation counts, timestamps, and summaries.")]
    async fn list_sessions(
        &self,
        Parameters(params): Parameters<ListSessionsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::list_sessions::handle(self, params).await
    }

    #[tool(description = "Trigger reindex from MCP. Specify file paths for incremental reindex, or set full=true for complete workspace reindex. Rebuilds the graph after indexing.")]
    async fn trigger_reindex(
        &self,
        Parameters(params): Parameters<ReindexParams>,
    ) -> Result<CallToolResult, ErrorData> {
        super::tools::reindex::handle(self, params).await
    }

    #[tool(description = "Detect and mark stale observations whose linked files have changed. Also cleans up observations past the TTL.")]
    async fn detect_staleness(&self) -> Result<CallToolResult, ErrorData> {
        super::tools::staleness::handle(self).await
    }

    #[tool(description = "Show the current vexp configuration: token budget, skeleton level, exclude patterns, memory settings, and more.")]
    async fn get_config(&self) -> Result<CallToolResult, ErrorData> {
        super::tools::get_config::handle(self).await
    }
}

#[tool_handler]
impl ServerHandler for VexpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "vexp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                description: Some("Local-first context engine for AI coding agents".into()),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Vexp is a local-first context engine for AI coding agents. It indexes \
                 source code into a graph of symbols and relationships, then serves \
                 token-efficient context over MCP.\n\n\
                 ## Quick start\n\
                 1. `workspace_setup` — detect project type, generate config\n\
                 2. `trigger_reindex` — index the codebase (full=true for first run)\n\
                 3. `get_context_capsule` — your primary tool: hybrid search with intent \
                    detection, returns full pivot files + skeletonized supporting files \
                    within a token budget\n\n\
                 ## When to use each tool\n\
                 - **Understand code**: `get_context_capsule` (broad), `get_skeleton` (single file), \
                   `query_nodes` (structured symbol search)\n\
                 - **Trace dependencies**: `get_impact_graph` (callers/callees), \
                   `search_logic_flow` (paths between symbols), `query_edges` (filter relationships)\n\
                 - **Inspect the index**: `index_status`, `graph_stats`, `list_files`, \
                   `get_unresolved_refs`, `get_config`\n\
                 - **Memory across sessions**: `save_observation`, `search_memory`, \
                   `get_session_context`, `list_sessions`, `detect_staleness`\n\
                 - **Maintain the index**: `trigger_reindex` (after file changes), \
                   `submit_lsp_edges` (supplement with LSP data)\n\n\
                 ## Tips\n\
                 - Prefer `get_context_capsule` over reading files directly — it respects \
                   token budgets and ranks by relevance.\n\
                 - Use `save_observation` to persist insights; they resurface in future capsules.\n\
                 - Use `query_nodes` with kind/visibility/exported_only filters to find specific \
                   symbols without reading entire files.\n\
                 - Use `get_impact_graph` with edge_kinds=['calls'] before refactoring to \
                   understand blast radius."
                    .to_string(),
            ),
        }
    }
}
