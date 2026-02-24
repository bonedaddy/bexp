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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ImpactParams {
    #[schemars(description = "Symbol name to analyze")]
    pub symbol: String,
    #[schemars(description = "Direction: 'callers', 'callees', or 'both'")]
    pub direction: Option<String>,
    #[schemars(description = "Maximum traversal depth (default: 3)")]
    pub depth: Option<usize>,
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

    #[tool(description = "Get the impact graph of a symbol showing callers, callees, or dependents up to N depth. Useful for understanding blast radius of changes.")]
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
                "Vexp is a context engine that provides token-efficient code context. \
                 Use get_context_capsule for the best results — it automatically selects \
                 relevant files and returns them within your token budget. Use get_skeleton \
                 for individual file summaries, get_impact_graph for dependency analysis, \
                 and search_logic_flow to trace execution paths."
                    .to_string(),
            ),
        }
    }
}
