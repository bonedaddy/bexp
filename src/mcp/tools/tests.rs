use crate::mcp::server::*;
use crate::mcp::test_helpers::TestServerBuilder;

const RUST_SOURCE: &str = "\
pub fn alpha() {
    beta();
}

pub fn beta() -> i32 {
    42
}

pub struct Config {
    pub name: String,
}

impl Config {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}
";

const RUST_SOURCE_2: &str = "\
fn consumer() {
    let _val = alpha();
}
";

fn server_with_rust_files() -> (
    super::super::server::BexpServer,
    crate::mcp::test_helpers::TempWorkspace,
) {
    TestServerBuilder::new()
        .with_file("lib.rs", RUST_SOURCE)
        .with_file("consumer.rs", RUST_SOURCE_2)
        .build()
}

fn empty_server() -> (
    super::super::server::BexpServer,
    crate::mcp::test_helpers::TempWorkspace,
) {
    TestServerBuilder::new().build()
}

// ─── Capsule tests ───

#[tokio::test]
async fn capsule_with_valid_query_returns_success() {
    let (server, _ws) = server_with_rust_files();
    let params = CapsuleParams {
        query: "alpha function".to_string(),
        token_budget: Some(4000),
        session_id: None,
        intent: None,
    };
    let result = super::capsule::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "capsule handle should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn capsule_with_empty_query_returns_error() {
    let (server, _ws) = server_with_rust_files();
    let params = CapsuleParams {
        query: String::new(),
        token_budget: None,
        session_id: None,
        intent: None,
    };
    let result = super::capsule::handle(&server, params).await;
    assert!(result.is_err(), "empty query should return error");
}

#[tokio::test]
async fn capsule_with_session_id() {
    let (server, _ws) = server_with_rust_files();
    let params = CapsuleParams {
        query: "Config struct".to_string(),
        token_budget: Some(2000),
        session_id: Some("test-session".to_string()),
        intent: Some("explore".to_string()),
    };
    let result = super::capsule::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Impact tests ───

#[tokio::test]
async fn impact_with_known_symbol() {
    let (server, _ws) = server_with_rust_files();
    let params = ImpactParams {
        symbol: "alpha".to_string(),
        direction: Some("both".to_string()),
        depth: Some(3),
        edge_kinds: None,
    };
    let result = super::impact::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "impact handle should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn impact_with_invalid_direction() {
    let (server, _ws) = server_with_rust_files();
    let params = ImpactParams {
        symbol: "alpha".to_string(),
        direction: Some("sideways".to_string()),
        depth: None,
        edge_kinds: None,
    };
    let result = super::impact::handle(&server, params).await;
    assert!(result.is_err(), "invalid direction should return error");
}

#[tokio::test]
async fn impact_with_empty_symbol() {
    let (server, _ws) = server_with_rust_files();
    let params = ImpactParams {
        symbol: String::new(),
        direction: None,
        depth: None,
        edge_kinds: None,
    };
    let result = super::impact::handle(&server, params).await;
    assert!(
        result.is_err(),
        "empty symbol should return validation error"
    );
}

#[tokio::test]
async fn impact_with_edge_kind_filter() {
    let (server, _ws) = server_with_rust_files();
    let params = ImpactParams {
        symbol: "alpha".to_string(),
        direction: Some("callees".to_string()),
        depth: Some(2),
        edge_kinds: Some(vec!["calls".to_string()]),
    };
    let result = super::impact::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Flow tests ───

#[tokio::test]
async fn flow_between_symbols() {
    let (server, _ws) = server_with_rust_files();
    let params = FlowParams {
        from_symbol: "alpha".to_string(),
        to_symbol: "beta".to_string(),
        max_depth: Some(5),
    };
    let result = super::flow::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "flow handle should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn flow_with_empty_from_symbol() {
    let (server, _ws) = server_with_rust_files();
    let params = FlowParams {
        from_symbol: String::new(),
        to_symbol: "beta".to_string(),
        max_depth: None,
    };
    let result = super::flow::handle(&server, params).await;
    assert!(result.is_err(), "empty from_symbol should return error");
}

#[tokio::test]
async fn flow_with_empty_to_symbol() {
    let (server, _ws) = server_with_rust_files();
    let params = FlowParams {
        from_symbol: "alpha".to_string(),
        to_symbol: String::new(),
        max_depth: None,
    };
    let result = super::flow::handle(&server, params).await;
    assert!(result.is_err(), "empty to_symbol should return error");
}

// ─── Query nodes tests ───

#[tokio::test]
async fn query_nodes_returns_matching_symbols() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryNodesParams {
        query: Some("alpha".to_string()),
        kind: None,
        file_path: None,
        visibility: None,
        exported_only: None,
        include_pagerank: None,
        limit: None,
    };
    let result = super::query_nodes::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn query_nodes_with_kind_filter() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryNodesParams {
        query: None,
        kind: Some("function".to_string()),
        file_path: None,
        visibility: None,
        exported_only: None,
        include_pagerank: None,
        limit: Some(10),
    };
    let result = super::query_nodes::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn query_nodes_with_limit_zero_fails() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryNodesParams {
        query: Some("alpha".to_string()),
        kind: None,
        file_path: None,
        visibility: None,
        exported_only: None,
        include_pagerank: None,
        limit: Some(0),
    };
    let result = super::query_nodes::handle(&server, params).await;
    assert!(result.is_err(), "limit=0 should return validation error");
}

#[tokio::test]
async fn query_nodes_with_limit_over_max_fails() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryNodesParams {
        query: None,
        kind: None,
        file_path: None,
        visibility: None,
        exported_only: None,
        include_pagerank: None,
        limit: Some(1001),
    };
    let result = super::query_nodes::handle(&server, params).await;
    assert!(result.is_err(), "limit>1000 should return validation error");
}

#[tokio::test]
async fn query_nodes_with_pagerank() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryNodesParams {
        query: Some("alpha".to_string()),
        kind: None,
        file_path: None,
        visibility: None,
        exported_only: None,
        include_pagerank: Some(true),
        limit: None,
    };
    let result = super::query_nodes::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn query_nodes_with_file_path_filter() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryNodesParams {
        query: None,
        kind: None,
        file_path: Some("lib.rs".to_string()),
        visibility: None,
        exported_only: None,
        include_pagerank: None,
        limit: None,
    };
    let result = super::query_nodes::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Query edges tests ───

#[tokio::test]
async fn query_edges_returns_relationships() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryEdgesParams {
        symbol: None,
        kind: None,
        min_confidence: None,
        direction: None,
        limit: None,
    };
    let result = super::query_edges::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn query_edges_with_limit_zero_fails() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryEdgesParams {
        symbol: None,
        kind: None,
        min_confidence: None,
        direction: None,
        limit: Some(0),
    };
    let result = super::query_edges::handle(&server, params).await;
    assert!(result.is_err(), "limit=0 should fail");
}

#[tokio::test]
async fn query_edges_with_limit_over_max_fails() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryEdgesParams {
        symbol: None,
        kind: None,
        min_confidence: None,
        direction: None,
        limit: Some(1001),
    };
    let result = super::query_edges::handle(&server, params).await;
    assert!(result.is_err(), "limit>1000 should fail");
}

#[tokio::test]
async fn query_edges_with_kind_filter() {
    let (server, _ws) = server_with_rust_files();
    let params = QueryEdgesParams {
        symbol: None,
        kind: Some("calls".to_string()),
        min_confidence: None,
        direction: None,
        limit: Some(50),
    };
    let result = super::query_edges::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Memory/Observation tests ───

#[tokio::test]
async fn save_observation_stores_content() {
    let (server, _ws) = server_with_rust_files();
    let params = ObserveParams {
        content: "alpha has a bug when called with negative inputs".to_string(),
        session_id: "test-session-1".to_string(),
        symbols: Some(vec!["alpha".to_string()]),
        files: None,
    };
    let result = super::observe::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "save_observation should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn save_observation_with_empty_content_fails() {
    let (server, _ws) = server_with_rust_files();
    let params = ObserveParams {
        content: String::new(),
        session_id: "test-session-1".to_string(),
        symbols: None,
        files: None,
    };
    let result = super::observe::handle(&server, params).await;
    assert!(
        result.is_err(),
        "empty content should return validation error"
    );
}

#[tokio::test]
async fn save_observation_without_symbols() {
    let (server, _ws) = server_with_rust_files();
    let params = ObserveParams {
        content: "general observation about the codebase".to_string(),
        session_id: "test-session-2".to_string(),
        symbols: None,
        files: None,
    };
    let result = super::observe::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Memory search tests ───

#[tokio::test]
async fn search_memory_returns_results() {
    let (server, _ws) = server_with_rust_files();

    // First save an observation
    let observe_params = ObserveParams {
        content: "alpha function handles the primary logic flow".to_string(),
        session_id: "search-test-session".to_string(),
        symbols: Some(vec!["alpha".to_string()]),
        files: None,
    };
    super::observe::handle(&server, observe_params)
        .await
        .unwrap();

    // Then search for it
    let params = MemorySearchParams {
        query: "alpha logic".to_string(),
        limit: Some(10),
        session_id: Some("search-test-session".to_string()),
    };
    let result = super::memory_search::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_memory_with_empty_query_fails() {
    let (server, _ws) = server_with_rust_files();
    let params = MemorySearchParams {
        query: String::new(),
        limit: None,
        session_id: None,
    };
    let result = super::memory_search::handle(&server, params).await;
    assert!(result.is_err(), "empty query should fail");
}

// ─── Session tests ───

#[tokio::test]
async fn get_session_context_returns_data() {
    let (server, _ws) = server_with_rust_files();

    // Save an observation first to create a session
    let observe_params = ObserveParams {
        content: "session context test observation".to_string(),
        session_id: "ctx-test-session".to_string(),
        symbols: None,
        files: None,
    };
    super::observe::handle(&server, observe_params)
        .await
        .unwrap();

    let params = SessionParams {
        session_id: Some("ctx-test-session".to_string()),
        include_previous: Some(false),
        previous_limit: None,
    };
    let result = super::session::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn get_session_context_with_no_session() {
    let (server, _ws) = empty_server();
    let params = SessionParams {
        session_id: None,
        include_previous: None,
        previous_limit: None,
    };
    let result = super::session::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── List sessions tests ───

#[tokio::test]
async fn list_sessions_returns_list() {
    let (server, _ws) = server_with_rust_files();

    // Create a session by saving an observation
    let observe_params = ObserveParams {
        content: "list sessions test observation".to_string(),
        session_id: "list-test-session".to_string(),
        symbols: None,
        files: None,
    };
    super::observe::handle(&server, observe_params)
        .await
        .unwrap();

    let params = ListSessionsParams { limit: Some(10) };
    let result = super::list_sessions::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn list_sessions_with_default_limit() {
    let (server, _ws) = empty_server();
    let params = ListSessionsParams { limit: None };
    let result = super::list_sessions::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn list_sessions_with_zero_limit_fails() {
    let (server, _ws) = empty_server();
    let params = ListSessionsParams { limit: Some(0) };
    let result = super::list_sessions::handle(&server, params).await;
    assert!(result.is_err(), "limit=0 should fail");
}

// ─── Index status tests ───

#[tokio::test]
async fn index_status_returns_counts() {
    let (server, _ws) = server_with_rust_files();
    let result = super::status::handle(&server).await;
    assert!(result.is_ok(), "status should succeed: {:?}", result.err());
}

#[tokio::test]
async fn index_status_empty_workspace() {
    let (server, _ws) = empty_server();
    let result = super::status::handle(&server).await;
    assert!(result.is_ok());
}

// ─── List files tests ───

#[tokio::test]
async fn list_files_returns_file_list() {
    let (server, _ws) = server_with_rust_files();
    let params = ListFilesParams {
        language: None,
        sort_by: None,
        limit: None,
    };
    let result = super::list_files::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn list_files_with_language_filter() {
    let (server, _ws) = server_with_rust_files();
    let params = ListFilesParams {
        language: Some("rust".to_string()),
        sort_by: None,
        limit: Some(10),
    };
    let result = super::list_files::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn list_files_with_sort() {
    let (server, _ws) = server_with_rust_files();
    let params = ListFilesParams {
        language: None,
        sort_by: Some("size".to_string()),
        limit: None,
    };
    let result = super::list_files::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Skeleton tests ───

#[tokio::test]
async fn get_skeleton_returns_content() {
    let (server, _ws) = server_with_rust_files();
    let params = SkeletonParams {
        file_path: "lib.rs".to_string(),
        level: Some("standard".to_string()),
    };
    let result = super::skeleton::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "skeleton should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn get_skeleton_minimal_level() {
    let (server, _ws) = server_with_rust_files();
    let params = SkeletonParams {
        file_path: "lib.rs".to_string(),
        level: Some("minimal".to_string()),
    };
    let result = super::skeleton::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn get_skeleton_path_traversal_rejected() {
    let (server, _ws) = server_with_rust_files();
    let params = SkeletonParams {
        file_path: "../../../etc/passwd".to_string(),
        level: None,
    };
    let result = super::skeleton::handle(&server, params).await;
    assert!(result.is_err(), "path traversal should be rejected");
}

// ─── Reindex tests ───

#[tokio::test]
async fn trigger_reindex_full() {
    let (server, _ws) = server_with_rust_files();
    let params = ReindexParams {
        files: None,
        full: Some(true),
    };
    let result = super::reindex::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "full reindex should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn trigger_reindex_specific_file() {
    let (server, _ws) = server_with_rust_files();
    let params = ReindexParams {
        files: Some(vec!["lib.rs".to_string()]),
        full: None,
    };
    let result = super::reindex::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Graph stats tests ───

#[tokio::test]
async fn graph_stats_returns_stats() {
    let (server, _ws) = server_with_rust_files();
    let params = GraphStatsParams {
        top_n: Some(10),
        kind: None,
    };
    let result = super::graph_stats::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "graph_stats should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn graph_stats_with_kind_filter() {
    let (server, _ws) = server_with_rust_files();
    let params = GraphStatsParams {
        top_n: Some(5),
        kind: Some("function".to_string()),
    };
    let result = super::graph_stats::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn graph_stats_empty_workspace() {
    let (server, _ws) = empty_server();
    let params = GraphStatsParams {
        top_n: None,
        kind: None,
    };
    let result = super::graph_stats::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Config tests ───

#[tokio::test]
async fn get_config_returns_configuration() {
    let (server, _ws) = empty_server();
    let result = super::get_config::handle(&server).await;
    assert!(
        result.is_ok(),
        "get_config should succeed: {:?}",
        result.err()
    );
}

// ─── Setup tests ───

#[tokio::test]
async fn workspace_setup_creates_config_files() {
    let (server, _ws) = empty_server();
    let params = SetupParams { force: Some(true) };
    let result = super::setup::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "workspace_setup should succeed: {:?}",
        result.err()
    );

    // Verify files were created
    assert!(_ws.path().join(".bexp/config.toml").exists());
    assert!(_ws.path().join(".bexp/.gitignore").exists());
}

#[tokio::test]
async fn workspace_setup_no_force_existing() {
    let (server, _ws) = empty_server();

    // First setup
    let params = SetupParams { force: Some(true) };
    super::setup::handle(&server, params).await.unwrap();

    // Second setup without force should say it already exists
    let params = SetupParams { force: None };
    let result = super::setup::handle(&server, params).await;
    assert!(result.is_ok());
}

// ─── Staleness tests ───

#[tokio::test]
async fn detect_staleness_runs_without_error() {
    let (server, _ws) = server_with_rust_files();
    let result = super::staleness::handle(&server).await;
    assert!(
        result.is_ok(),
        "staleness should succeed: {:?}",
        result.err()
    );
}

// ─── LSP edges tests ───

#[tokio::test]
async fn submit_lsp_edges_with_empty_list() {
    let (server, _ws) = server_with_rust_files();
    let params = LspEdgesParams { edges: vec![] };
    let result = super::lsp::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn submit_lsp_edges_with_unresolved_symbols() {
    let (server, _ws) = server_with_rust_files();
    let params = LspEdgesParams {
        edges: vec![LspEdge {
            source_qualified_name: "nonexistent::source".to_string(),
            target_qualified_name: "nonexistent::target".to_string(),
            kind: "calls".to_string(),
        }],
    };
    let result = super::lsp::handle(&server, params).await;
    assert!(
        result.is_ok(),
        "unresolved symbols should be skipped, not error"
    );
}

// ─── Unresolved refs tests ───

#[tokio::test]
async fn get_unresolved_refs_returns_results() {
    let (server, _ws) = server_with_rust_files();
    let params = UnresolvedRefsParams {
        file_path: None,
        limit: None,
    };
    let result = super::unresolved::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn get_unresolved_refs_with_file_filter() {
    let (server, _ws) = server_with_rust_files();
    let params = UnresolvedRefsParams {
        file_path: Some("lib.rs".to_string()),
        limit: Some(10),
    };
    let result = super::unresolved::handle(&server, params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn get_unresolved_refs_limit_zero_fails() {
    let (server, _ws) = server_with_rust_files();
    let params = UnresolvedRefsParams {
        file_path: None,
        limit: Some(0),
    };
    let result = super::unresolved::handle(&server, params).await;
    assert!(result.is_err(), "limit=0 should fail");
}
