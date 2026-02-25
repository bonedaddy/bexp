pub mod builder;
pub mod centrality;
pub mod traversal;

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use rusqlite::Connection;

use crate::db::queries;
use crate::error::{Result, bexpError};
use crate::types::{EdgeKind, NodeKind};

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub db_id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: NodeKind,
    pub file_id: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GraphEdge {
    pub kind: EdgeKind,
    pub confidence: f64,
}

pub struct GraphEngine {
    graph: RwLock<DiGraph<GraphNode, GraphEdge>>,
    id_to_index: RwLock<HashMap<i64, NodeIndex>>,
    pagerank: RwLock<HashMap<NodeIndex, f64>>,
}

impl Default for GraphEngine {
    fn default() -> Self {
        Self::new()
    }
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

        *self.graph.write().map_err(|_| bexpError::Graph("lock poisoned".into()))? = graph;
        *self.id_to_index.write().map_err(|_| bexpError::Graph("lock poisoned".into()))? = id_map;
        *self.pagerank.write().map_err(|_| bexpError::Graph("lock poisoned".into()))? = pagerank;

        Ok(())
    }

    pub fn rebuild_from_db(&self, conn: &Connection) -> Result<()> {
        self.build_from_db(conn)
    }

    pub fn node_count(&self) -> usize {
        self.graph.read().map(|g| g.node_count()).unwrap_or(0)
    }

    pub fn edge_count(&self) -> usize {
        self.graph.read().map(|g| g.edge_count()).unwrap_or(0)
    }

    pub fn get_pagerank(&self, db_id: i64) -> f64 {
        let id_map = match self.id_to_index.read() {
            Ok(m) => m,
            Err(_) => return 0.0,
        };
        let pr = match self.pagerank.read() {
            Ok(p) => p,
            Err(_) => return 0.0,
        };
        id_map
            .get(&db_id)
            .and_then(|idx| pr.get(idx))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn impact_graph(
        &self,
        symbol: &str,
        direction: &str,
        depth: usize,
        edge_kinds: Option<&[String]>,
    ) -> Result<String> {
        let graph = self.graph.read().map_err(|_| bexpError::Graph("lock poisoned".into()))?;

        // Find the node
        let node_idx = graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == symbol
                    || node.qualified_name.as_deref() == Some(symbol)
            })
            .ok_or_else(|| bexpError::NotFound(format!("Symbol not found: {}", symbol)))?;

        let result = match direction {
            "callers" => traversal::get_callers(&graph, node_idx, depth, edge_kinds),
            "callees" => traversal::get_callees(&graph, node_idx, depth, edge_kinds),
            "both" => {
                let mut result = traversal::get_callers(&graph, node_idx, depth, edge_kinds);
                result.push_str("\n---\n\n");
                result.push_str(&traversal::get_callees(&graph, node_idx, depth, edge_kinds));
                result
            }
            _ => return Err(bexpError::Graph(format!("Invalid direction: {}", direction))),
        };

        Ok(result)
    }

    pub fn find_paths(&self, from: &str, to: &str, max_depth: usize) -> Result<String> {
        let graph = self.graph.read().map_err(|_| bexpError::Graph("lock poisoned".into()))?;

        let from_idx = graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == from || node.qualified_name.as_deref() == Some(from)
            })
            .ok_or_else(|| bexpError::NotFound(format!("Source symbol not found: {}", from)))?;

        let to_idx = graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == to || node.qualified_name.as_deref() == Some(to)
            })
            .ok_or_else(|| bexpError::NotFound(format!("Target symbol not found: {}", to)))?;

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
                    node.kind, // Display impl delegates to as_str()
                    arrow
                ));
            }
            output.push('\n');
        }

        Ok(output)
    }

    pub fn find_node_index_by_name(&self, name: &str) -> Option<i64> {
        let graph = self.graph.read().ok()?;
        graph
            .node_indices()
            .find(|&idx| {
                let node = &graph[idx];
                node.name == name || node.qualified_name.as_deref() == Some(name)
            })
            .map(|idx| graph[idx].db_id)
    }

    pub fn get_top_pagerank(
        &self,
        n: usize,
        kind_filter: Option<&str>,
    ) -> Vec<(String, String, f64)> {
        let graph = match self.graph.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let pr = match self.pagerank.read() {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };

        let mut entries: Vec<(String, String, f64)> = pr
            .iter()
            .filter_map(|(idx, &score)| {
                let node = &graph[*idx];
                if let Some(kind) = kind_filter {
                    if node.kind.as_str() != kind {
                        return None;
                    }
                }
                Some((
                    node.qualified_name.clone().unwrap_or_else(|| node.name.clone()),
                    node.kind.as_str().to_string(),
                    score,
                ))
            })
            .collect();

        entries.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        entries.truncate(n);
        entries
    }

    pub fn get_edge_kind_counts(&self) -> Vec<(String, usize)> {
        let graph = match self.graph.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut counts: HashMap<String, usize> = HashMap::new();
        for edge in graph.edge_weights() {
            *counts.entry(edge.kind.as_str().to_string()).or_insert(0) += 1;
        }
        let mut result: Vec<(String, usize)> = counts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }

    /// 1-hop BFS from pivot nodes along relevant edge kinds.
    /// Returns db_ids of neighbor nodes NOT already in the included set.
    pub fn get_bridge_candidates(
        &self,
        pivot_node_ids: &HashSet<i64>,
        included_node_ids: &HashSet<i64>,
    ) -> Vec<i64> {
        let graph = match self.graph.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let id_map = match self.id_to_index.read() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };

        let bridge_edge_kinds = ["calls", "imports", "implements", "extends"];
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        for &db_id in pivot_node_ids {
            let idx = match id_map.get(&db_id) {
                Some(&idx) => idx,
                None => continue,
            };

            // Check both outgoing and incoming edges
            for direction in [Direction::Outgoing, Direction::Incoming] {
                for neighbor in graph.neighbors_directed(idx, direction) {
                    let neighbor_db_id = graph[neighbor].db_id;

                    // Skip if already included or already seen
                    if included_node_ids.contains(&neighbor_db_id)
                        || !seen.insert(neighbor_db_id)
                    {
                        continue;
                    }

                    // Check that at least one edge to/from this neighbor is a relevant kind
                    let has_relevant_edge = match direction {
                        Direction::Outgoing => graph
                            .edges_connecting(idx, neighbor)
                            .any(|e| bridge_edge_kinds.contains(&e.weight().kind.as_str())),
                        Direction::Incoming => graph
                            .edges_connecting(neighbor, idx)
                            .any(|e| bridge_edge_kinds.contains(&e.weight().kind.as_str())),
                    };

                    if has_relevant_edge {
                        candidates.push(neighbor_db_id);
                    }
                }
            }
        }

        candidates
    }

    /// Incrementally update the graph for changed files instead of full rebuild.
    /// Falls back to full rebuild if >20% of nodes are affected.
    pub fn incremental_update(
        &self,
        conn: &Connection,
        changed_file_ids: &[i64],
    ) -> Result<()> {
        let total_nodes = self.node_count();

        // Get nodes belonging to changed files
        let changed_nodes = queries::get_nodes_for_files(conn, changed_file_ids)?;

        // If >20% of nodes affected, fall back to full rebuild
        if total_nodes > 0 && changed_nodes.len() * 5 > total_nodes {
            tracing::info!(
                "Incremental update: {} changed nodes > 20% of {} total, doing full rebuild",
                changed_nodes.len(),
                total_nodes
            );
            return self.build_from_db(conn);
        }

        let mut graph = self.graph.write().map_err(|_| bexpError::Graph("lock poisoned".into()))?;
        let mut id_map = self.id_to_index.write().map_err(|_| bexpError::Graph("lock poisoned".into()))?;

        // Collect indices to remove: all nodes belonging to changed files
        let changed_file_set: HashSet<i64> = changed_file_ids.iter().copied().collect();
        let mut indices_to_remove: Vec<NodeIndex> = Vec::new();

        for (_, &idx) in id_map.iter() {
            if graph.node_weight(idx).is_some_and(|n| changed_file_set.contains(&n.file_id)) {
                indices_to_remove.push(idx);
            }
        }

        // Remove in reverse index order to avoid invalidation from swap-remove
        indices_to_remove.sort_by_key(|idx| std::cmp::Reverse(idx.index()));

        for idx in &indices_to_remove {
            if let Some(node) = graph.node_weight(*idx) {
                id_map.remove(&node.db_id);
            }
            graph.remove_node(*idx);
        }

        // After removal, some indices may have been swapped. Rebuild id_map for remaining nodes.
        id_map.clear();
        for idx in graph.node_indices() {
            id_map.insert(graph[idx].db_id, idx);
        }

        // Re-add nodes for changed files from DB
        let new_nodes = queries::get_nodes_for_files(conn, changed_file_ids)?;
        for node in &new_nodes {
            let idx = graph.add_node(GraphNode {
                db_id: node.id,
                name: node.name.clone(),
                qualified_name: node.qualified_name.clone(),
                kind: NodeKind::parse(&node.kind).unwrap_or(NodeKind::External),
                file_id: node.file_id,
            });
            id_map.insert(node.id, idx);
        }

        // Re-add edges touching any new node
        let new_node_ids: Vec<i64> = new_nodes.iter().map(|n| n.id).collect();
        let edges = queries::get_edges_for_nodes(conn, &new_node_ids)?;
        for edge in &edges {
            if let (Some(&src), Some(&tgt)) = (
                id_map.get(&edge.source_node_id),
                id_map.get(&edge.target_node_id),
            ) {
                graph.add_edge(
                    src,
                    tgt,
                    GraphEdge {
                        kind: EdgeKind::parse(&edge.kind).unwrap_or(EdgeKind::Calls),
                        confidence: edge.confidence,
                    },
                );
            }
        }

        drop(graph);
        drop(id_map);

        // Recompute PageRank (global property, must be full)
        let graph_ref = self.graph.read().map_err(|_| bexpError::Graph("lock poisoned".into()))?;
        let pagerank = centrality::compute_pagerank(&graph_ref, 0.85, 20, 1e-6);
        drop(graph_ref);
        *self.pagerank.write().map_err(|_| bexpError::Graph("lock poisoned".into()))? = pagerank;

        tracing::info!(
            "Incremental graph update: removed {} old nodes, added {} new nodes",
            indices_to_remove.len(),
            new_nodes.len()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup_graph_db() -> (Database, GraphEngine) {
        let db = Database::open_test().unwrap();
        let graph = GraphEngine::new();
        (db, graph)
    }

    fn insert_file(conn: &Connection, path: &str) -> i64 {
        queries::insert_file(conn, path, "rust", "hash", 0, 100).unwrap()
    }

    fn insert_node(conn: &Connection, file_id: i64, name: &str) -> i64 {
        queries::insert_node(conn, file_id, "function", name, None, None, None, 1, 10, 0, 0, Some("pub"), true, None).unwrap()
    }

    #[test]
    fn build_from_db_loads_nodes_and_edges() {
        let (db, graph) = setup_graph_db();
        let conn = db.writer();

        let f1 = insert_file(&conn, "a.rs");
        let f2 = insert_file(&conn, "b.rs");
        let n1 = insert_node(&conn, f1, "foo");
        let n2 = insert_node(&conn, f2, "bar");
        queries::insert_edge(&conn, n1, n2, "calls", 0.9, None).unwrap();
        drop(conn);

        graph.build_from_db(&db.reader()).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn incremental_update_removes_and_readds_nodes() {
        let (db, graph) = setup_graph_db();
        let conn = db.writer();

        let f1 = insert_file(&conn, "a.rs");
        let f2 = insert_file(&conn, "b.rs");
        let f3 = insert_file(&conn, "c.rs");
        let n1 = insert_node(&conn, f1, "foo");
        let n2 = insert_node(&conn, f2, "bar");
        let n3 = insert_node(&conn, f3, "baz");
        queries::insert_edge(&conn, n1, n2, "calls", 0.9, None).unwrap();
        queries::insert_edge(&conn, n2, n3, "calls", 0.9, None).unwrap();
        drop(conn);

        graph.build_from_db(&db.reader()).unwrap();
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);

        // Re-insert file b with different node (insert_file upserts and cascades old nodes)
        let f2_new;
        {
            let conn = db.writer();
            f2_new = queries::insert_file(&conn, "b.rs", "rust", "hash2", 1, 100).unwrap();
            let n2_new = insert_node(&conn, f2_new, "bar_new");
            queries::insert_edge(&conn, n1, n2_new, "calls", 0.8, None).unwrap();
        }

        // Incremental update for file b (use the new file_id)
        graph.incremental_update(&db.reader(), &[f2_new]).unwrap();

        // Should still have 3 nodes (old bar removed, new bar_new added)
        assert_eq!(graph.node_count(), 3);
        // Edge from n1 -> n2_new should exist, n2 -> n3 should be gone
        assert!(graph.edge_count() >= 1);
    }

    #[test]
    fn incremental_update_falls_back_to_full_rebuild_when_large_change() {
        let (db, graph) = setup_graph_db();
        let conn = db.writer();

        // Only 2 nodes total
        let f1 = insert_file(&conn, "a.rs");
        let f2 = insert_file(&conn, "b.rs");
        let _n1 = insert_node(&conn, f1, "foo");
        let _n2 = insert_node(&conn, f2, "bar");
        drop(conn);

        graph.build_from_db(&db.reader()).unwrap();
        assert_eq!(graph.node_count(), 2);

        // Change both files -> >20% threshold
        // (Since both files changed, that's 100% of nodes)
        graph.incremental_update(&db.reader(), &[f1, f2]).unwrap();
        assert_eq!(graph.node_count(), 2); // Still 2 after full rebuild
    }

    #[test]
    fn bridge_candidates_finds_intermediate_nodes() {
        let (db, graph) = setup_graph_db();
        let conn = db.writer();

        let f1 = insert_file(&conn, "a.rs");
        let f2 = insert_file(&conn, "b.rs");
        let f3 = insert_file(&conn, "c.rs");

        // A -> B -> C
        let a = insert_node(&conn, f1, "a_func");
        let b = insert_node(&conn, f2, "b_func");
        let c = insert_node(&conn, f3, "c_func");
        queries::insert_edge(&conn, a, b, "calls", 0.9, None).unwrap();
        queries::insert_edge(&conn, b, c, "calls", 0.9, None).unwrap();
        drop(conn);

        graph.build_from_db(&db.reader()).unwrap();

        // Pivots are {A, C}, should find B as bridge
        let pivots: HashSet<i64> = [a, c].into_iter().collect();
        let included: HashSet<i64> = pivots.clone();
        let bridges = graph.get_bridge_candidates(&pivots, &included);

        assert!(bridges.contains(&b), "Bridge candidates should include B");
    }

    #[test]
    fn bridge_candidates_excludes_already_included() {
        let (db, graph) = setup_graph_db();
        let conn = db.writer();

        let f1 = insert_file(&conn, "a.rs");
        let f2 = insert_file(&conn, "b.rs");

        let a = insert_node(&conn, f1, "a_func");
        let b = insert_node(&conn, f2, "b_func");
        queries::insert_edge(&conn, a, b, "calls", 0.9, None).unwrap();
        drop(conn);

        graph.build_from_db(&db.reader()).unwrap();

        // Both A and B already included
        let pivots: HashSet<i64> = [a].into_iter().collect();
        let included: HashSet<i64> = [a, b].into_iter().collect();
        let bridges = graph.get_bridge_candidates(&pivots, &included);

        assert!(bridges.is_empty(), "B should not be a bridge candidate since it's already included");
    }
}
