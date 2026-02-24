use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use rusqlite::Connection;

use crate::db::queries;
use crate::error::Result;

use super::{GraphEdge, GraphNode};

pub fn build_graph(
    conn: &Connection,
) -> Result<(DiGraph<GraphNode, GraphEdge>, HashMap<i64, NodeIndex>)> {
    let mut graph = DiGraph::new();
    let mut id_map = HashMap::new();

    // Load all nodes
    let nodes = queries::get_all_nodes(conn)?;
    for node in &nodes {
        let idx = graph.add_node(GraphNode {
            db_id: node.id,
            name: node.name.clone(),
            qualified_name: node.qualified_name.clone(),
            kind: node.kind.clone(),
            file_id: node.file_id,
        });
        id_map.insert(node.id, idx);
    }

    // Load all edges
    let edges = queries::get_all_edges(conn)?;
    for edge in &edges {
        if let (Some(&src), Some(&tgt)) = (
            id_map.get(&edge.source_node_id),
            id_map.get(&edge.target_node_id),
        ) {
            graph.add_edge(
                src,
                tgt,
                GraphEdge {
                    kind: edge.kind.clone(),
                    confidence: edge.confidence,
                },
            );
        }
    }

    tracing::info!(
        "Graph built: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    Ok((graph, id_map))
}
