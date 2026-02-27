pub mod budget;
pub mod cache;
pub mod generator;
pub mod intent;
pub mod search;

use std::sync::Arc;

use crate::config::BexpConfig;
use crate::db::{queries, Database};
use crate::error::{BexpError, Result};
use crate::graph::GraphEngine;
use crate::memory::MemoryService;
use crate::skeleton::Skeletonizer;

use cache::CapsuleCache;

pub struct CapsuleGenerator {
    db: Arc<Database>,
    config: Arc<BexpConfig>,
    graph: Arc<GraphEngine>,
    skeletonizer: Arc<Skeletonizer>,
    memory: Arc<MemoryService>,
    cache: CapsuleCache,
}

impl CapsuleGenerator {
    pub fn new(
        db: Arc<Database>,
        config: Arc<BexpConfig>,
        graph: Arc<GraphEngine>,
        skeletonizer: Arc<Skeletonizer>,
        memory: Arc<MemoryService>,
    ) -> Self {
        let cache = CapsuleCache::new(config.capsule_cache_size, config.capsule_cache_ttl_secs);
        Self {
            db,
            config,
            graph,
            skeletonizer,
            memory,
            cache,
        }
    }

    pub fn generate(
        &self,
        query: &str,
        token_budget: usize,
        session_id: Option<&str>,
        intent_override: Option<&str>,
    ) -> Result<String> {
        self.generate_with_cross_workspace(query, token_budget, session_id, intent_override, true)
    }

    pub fn generate_with_cross_workspace(
        &self,
        query: &str,
        token_budget: usize,
        session_id: Option<&str>,
        intent_override: Option<&str>,
        cross_workspace: bool,
    ) -> Result<String> {
        tracing::debug!(
            query = query,
            token_budget = token_budget,
            "Generating capsule"
        );
        // 1. Detect intent (or use override)
        let intent = if let Some(name) = intent_override {
            match name {
                "debug" => crate::types::Intent::Debug,
                "blast_radius" => crate::types::Intent::BlastRadius,
                "modify" => crate::types::Intent::Modify,
                "explore" => crate::types::Intent::Explore,
                _ => {
                    return Err(BexpError::Config(format!(
                        "Invalid intent override '{name}'. Valid values: debug, blast_radius, modify, explore"
                    )));
                }
            }
        } else {
            intent::detect_intent(query)
        };
        tracing::debug!(intent = ?intent, "Detected intent");

        // Check cache (only for non-session queries since memory context varies)
        let generation = match self
            .db
            .reader()
            .and_then(|r| queries::get_index_generation(&r))
        {
            Ok(g) => g,
            Err(e) => {
                tracing::error!(error = %e, "Failed to read index generation; bypassing cache");
                u64::MAX
            }
        };
        if session_id.is_none() {
            if let Some(cached) = self
                .cache
                .get(query, token_budget, intent.as_str(), generation)
            {
                tracing::debug!(query = query, "Cache hit for capsule query");
                return Ok(cached);
            }
        }

        // 2. Hybrid search for relevant nodes
        let reader = self.db.reader()?;
        let t_search = std::time::Instant::now();

        // Open external workspace DBs if cross-workspace is enabled
        let external_dbs: Vec<(String, rusqlite::Connection)> = if cross_workspace
            && !self.config.workspace_group.is_empty()
        {
            self.config
                    .workspace_group
                    .iter()
                    .filter_map(|ws_path| {
                        let ws_name = std::path::Path::new(ws_path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| ws_path.clone());
                        match crate::workspace::open_external_db(ws_path) {
                            Ok(conn) => Some((ws_name, conn)),
                            Err(e) => {
                                tracing::debug!(workspace = %ws_path, error = %e, "Cannot open external workspace for search");
                                None
                            }
                        }
                    })
                    .collect()
        } else {
            Vec::new()
        };

        let ext_ref: Option<&[(String, rusqlite::Connection)]> = if external_dbs.is_empty() {
            None
        } else {
            Some(&external_dbs)
        };

        let search_results =
            search::hybrid_search_with_external(&reader, &self.graph, query, &intent, 50, ext_ref)?;
        let search_ms = t_search.elapsed().as_millis();

        tracing::debug!(
            search_results = search_results.len(),
            elapsed_ms = search_ms,
            "Hybrid search complete"
        );

        if search_results.is_empty() {
            return Ok("No relevant code found for the given query.".to_string());
        }

        // Separate local and external search results
        let (local_results, external_results): (Vec<_>, Vec<_>) = search_results
            .into_iter()
            .partition(|r| r.workspace.is_none());

        // 3. Allocate token budget
        let memory_budget = token_budget * self.config.memory_budget_pct / 100;
        let code_budget = token_budget - memory_budget;

        // 4. Select pivots and supporting files (local results only)
        let t_budget = std::time::Instant::now();
        let allocation = budget::allocate(
            &reader,
            &self.skeletonizer,
            &local_results,
            code_budget,
            &intent,
            Some(&self.graph),
            &self.config,
        )?;
        let budget_ms = t_budget.elapsed().as_millis();

        tracing::debug!(
            pivots = allocation.pivots.len(),
            bridges = allocation.bridges.len(),
            skeletons = allocation.skeletons.len(),
            total_tokens = allocation.total_tokens,
            elapsed_ms = budget_ms,
            "Budget allocation complete"
        );

        // 5. Assemble capsule
        let t_assemble = std::time::Instant::now();
        let mut output = generator::assemble_capsule(&reader, &allocation, query, &intent)?;
        let assemble_ms = t_assemble.elapsed().as_millis();
        tracing::info!(
            search_ms = search_ms,
            budget_ms = budget_ms,
            assemble_ms = assemble_ms,
            total_ms = t_search.elapsed().as_millis(),
            query = query,
            "Capsule pipeline timing"
        );

        // 5b. Add cross-workspace external results section
        if !external_results.is_empty() {
            output.push_str("\n---\n\n## External References\n\n");
            let mut by_workspace: std::collections::HashMap<String, Vec<&search::SearchResult>> =
                std::collections::HashMap::new();
            for r in &external_results {
                if let Some(ws) = &r.workspace {
                    by_workspace.entry(ws.clone()).or_default().push(r);
                }
            }
            let mut ws_names: Vec<_> = by_workspace.keys().cloned().collect();
            ws_names.sort();
            for ws_name in ws_names {
                let results = &by_workspace[&ws_name];
                output.push_str(&format!("### External: {ws_name}\n\n"));
                for r in results {
                    let qname = r.qualified_name.as_deref().unwrap_or(&r.name);
                    output.push_str(&format!(
                        "- `{ws_name}/{}` ({}) — score: {:.2}\n",
                        qname, r.kind, r.score
                    ));
                }
                output.push('\n');
            }
        }

        // 6. Add memory context if session provided
        if let Some(sid) = session_id {
            let memory_context = match self.memory.search(query, 5, Some(sid)) {
                Ok(ctx) => ctx,
                Err(e) => {
                    tracing::error!(
                        session_id = sid,
                        error = %e,
                        "Memory search failed; capsule generated without session context"
                    );
                    String::new()
                }
            };
            if !memory_context.is_empty() {
                output.push_str("\n---\n\n# Relevant Observations\n\n");
                output.push_str(&memory_context);
            }
        }

        // Cache result (only non-session queries)
        if session_id.is_none() {
            self.cache.put(
                query,
                token_budget,
                intent.as_str(),
                generation,
                output.clone(),
            );
        }

        Ok(output)
    }
}
