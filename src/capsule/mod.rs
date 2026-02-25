pub mod budget;
pub mod cache;
pub mod generator;
pub mod intent;
pub mod search;

use std::sync::Arc;

use crate::config::bexpConfig;
use crate::db::{queries, Database};
use crate::error::Result;
use crate::graph::GraphEngine;
use crate::memory::MemoryService;
use crate::skeleton::Skeletonizer;

use cache::CapsuleCache;

pub struct CapsuleGenerator {
    db: Arc<Database>,
    config: Arc<bexpConfig>,
    graph: Arc<GraphEngine>,
    skeletonizer: Arc<Skeletonizer>,
    memory: Arc<MemoryService>,
    cache: CapsuleCache,
}

impl CapsuleGenerator {
    pub fn new(
        db: Arc<Database>,
        config: Arc<bexpConfig>,
        graph: Arc<GraphEngine>,
        skeletonizer: Arc<Skeletonizer>,
        memory: Arc<MemoryService>,
    ) -> Self {
        Self {
            db,
            config,
            graph,
            skeletonizer,
            memory,
            cache: CapsuleCache::new(),
        }
    }

    pub fn generate(
        &self,
        query: &str,
        token_budget: usize,
        session_id: Option<&str>,
        intent_override: Option<&str>,
    ) -> Result<String> {
        // 1. Detect intent (or use override)
        let intent = if let Some(name) = intent_override {
            match name {
                "debug" => crate::types::Intent::Debug,
                "blast_radius" => crate::types::Intent::BlastRadius,
                "modify" => crate::types::Intent::Modify,
                "explore" => crate::types::Intent::Explore,
                _ => intent::detect_intent(query),
            }
        } else {
            intent::detect_intent(query)
        };
        tracing::debug!("Detected intent: {:?}", intent);

        // Check cache (only for non-session queries since memory context varies)
        let generation = queries::get_index_generation(&self.db.reader()).unwrap_or(0);
        if session_id.is_none() {
            if let Some(cached) = self.cache.get(query, token_budget, intent.as_str(), generation) {
                tracing::debug!("Cache hit for query: {}", query);
                return Ok(cached);
            }
        }

        // 2. Hybrid search for relevant nodes
        let reader = self.db.reader();
        let search_results = search::hybrid_search(
            &reader,
            &self.graph,
            query,
            &intent,
            50,
        )?;

        if search_results.is_empty() {
            return Ok("No relevant code found for the given query.".to_string());
        }

        // 3. Allocate token budget
        let memory_budget = (token_budget as f64 * self.config.memory_budget_pct) as usize;
        let code_budget = token_budget - memory_budget;

        // 4. Select pivots and supporting files
        let allocation = budget::allocate(
            &reader,
            &self.skeletonizer,
            &search_results,
            code_budget,
            self.config.default_skeleton_level,
            Some(&self.graph),
        )?;

        // 5. Assemble capsule
        let mut output = generator::assemble_capsule(
            &reader,
            &allocation,
            query,
            &intent,
        )?;

        // 6. Add memory context if session provided
        if let Some(sid) = session_id {
            let memory_context = self.memory.search(query, 5, Some(sid)).unwrap_or_default();
            if !memory_context.is_empty() {
                output.push_str("\n---\n\n# Relevant Observations\n\n");
                output.push_str(&memory_context);
            }
        }

        // Cache result (only non-session queries)
        if session_id.is_none() {
            self.cache.put(query, token_budget, intent.as_str(), generation, output.clone());
        }

        Ok(output)
    }
}
