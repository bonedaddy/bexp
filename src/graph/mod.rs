pub mod builder;
pub mod centrality;
pub mod traversal;

use std::collections::HashMap;
use std::sync::RwLock;

use petgraph::graph::{DiGraph, NodeIndex};
use rusqlite::Connection;

use crate::error::{Result, VexpError};

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub db_id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: String,
    pub file_id: i64,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub kind: String,
    pub confidence: f64,
}

pub struct GraphEngine {
    graph: RwLock<DiGraph<GraphNode, GraphEdge>>,
    id_to_index: RwLock<HashMap<i64, NodeIndex>>,
    pagerank: RwLock<HashMap<NodeIndex, f64>>,
}

impl GraphEngine {
    pub fn new() -> Self {
        Self {
            graph: RwLock::new(DiGraph::new()),
            id_to_index: RwLock::new(HashMap::new()),
            pagerank: RwLock::new(HashMap::new()),
        }
    }

    pub fn build_from_db(&self, conn: &Connection) -> Result<()> {
        let (graph, id_map) = builder::build_graph(conn)?;

        let pagerank = centrality::compute_pagerank(&graph, 0.85, 20, 1e-6);

        *self.graph.write().unwrap() = graph;
        *self.id_to_index.write().unwrap() = id_map;
        *self.pagerank.write().unwrap() = pagerank;

        Ok(())
    }

    pub fn rebuild_from_db(&self, conn: &Connection) -> Result<()> {
        self.build_from_db(conn)
    }

    pub fn node_count(&self) -> usize {
        self.graph.read().unwrap().node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.read().unwrap().edge_count()
    }

    pub fn get_pagerank(&self, db_id: i64) -> f64 {
        let id_map = self.id_to_index.read().unwrap();
        let pr = self.pagerank.read().unwrap();
        id_map
            .get(&db_id)
            .and_then(|idx| pr.get(idx))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn impact_graph(&self, symbol: &str, direction: &str, depth: usize) -> Result<String> {
        let graph = self.graph.read().unwrap();
        let _id_map = self.id_to_index.read().unwrap();

        // Find the node
        let node_idx = graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == symbol
                    || node.qualified_name.as_deref() == Some(symbol)
            })
            .ok_or_else(|| VexpError::NotFound(format!("Symbol not found: {}", symbol)))?;

        let result = match direction {
            "callers" => traversal::get_callers(&graph, node_idx, depth),
            "callees" => traversal::get_callees(&graph, node_idx, depth),
            "both" => {
                let mut result = traversal::get_callers(&graph, node_idx, depth);
                result.push_str("\n---\n\n");
                result.push_str(&traversal::get_callees(&graph, node_idx, depth));
                result
            }
            _ => return Err(VexpError::Graph(format!("Invalid direction: {}", direction))),
        };

        Ok(result)
    }

    pub fn find_paths(&self, from: &str, to: &str, max_depth: usize) -> Result<String> {
        let graph = self.graph.read().unwrap();

        let from_idx = graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == from || node.qualified_name.as_deref() == Some(from)
            })
            .ok_or_else(|| VexpError::NotFound(format!("Source symbol not found: {}", from)))?;

        let to_idx = graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == to || node.qualified_name.as_deref() == Some(to)
            })
            .ok_or_else(|| VexpError::NotFound(format!("Target symbol not found: {}", to)))?;

        let paths = traversal::find_all_paths(&graph, from_idx, to_idx, max_depth);

        if paths.is_empty() {
            return Ok(format!(
                "No paths found from `{}` to `{}` within {} hops.",
                from, to, max_depth
            ));
        }

        let mut output = format!("# Paths from `{}` to `{}`\n\n", from, to);
        for (i, path) in paths.iter().enumerate() {
            output.push_str(&format!("## Path {} ({} hops)\n\n", i + 1, path.len() - 1));
            for (j, idx) in path.iter().enumerate() {
                let node = &graph[*idx];
                let arrow = if j < path.len() - 1 { " →" } else { "" };
                output.push_str(&format!(
                    "- `{}` ({}){}\n",
                    node.qualified_name.as_deref().unwrap_or(&node.name),
                    node.kind,
                    arrow
                ));
            }
            output.push('\n');
        }

        Ok(output)
    }

    pub fn find_node_index_by_name(&self, name: &str) -> Option<i64> {
        let graph = self.graph.read().unwrap();
        graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == name || node.qualified_name.as_deref() == Some(name)
            })
            .map(|idx| graph[idx].db_id)
    }
}
