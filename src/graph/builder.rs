use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use rusqlite::Connection;

use crate::error::Result;
use crate::db::queries;
use crate::types::{EdgeKind, NodeKind};

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
            kind: NodeKind::parse(&node.kind).unwrap_or(NodeKind::External),
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
                    kind: EdgeKind::parse(&edge.kind).unwrap_or(EdgeKind::Calls),
                    confidence: edge.confidence,
                },
            );
        }
    }

    // Load cross-workspace edges as synthetic nodes
    let cross_ws_count = load_cross_workspace_edges(conn, &mut graph, &mut id_map);

    tracing::info!(
        "Graph built: {} nodes, {} edges (incl. {} cross-workspace)",
        graph.node_count(),
        graph.edge_count(),
        cross_ws_count,
    );

    Ok((graph, id_map))
}

/// Load cross-workspace edges from the database. Creates synthetic graph nodes
/// for external targets (with file_id = -1) and edges connecting them.
/// Returns the number of cross-workspace edges added.
fn load_cross_workspace_edges(
    conn: &Connection,
    graph: &mut DiGraph<GraphNode, GraphEdge>,
    id_map: &mut HashMap<i64, NodeIndex>,
) -> usize {
    let mut stmt = match conn.prepare(
        "SELECT source_node_id, target_workspace, target_qualified_name, kind, confidence
         FROM cross_workspace_edges",
    ) {
        Ok(s) => s,
        Err(_) => return 0, // Table may not exist in old DBs
    };

    // Track synthetic nodes by (workspace, qualified_name) to avoid duplicates
    let mut synthetic_nodes: HashMap<(String, String), NodeIndex> = HashMap::new();
    let mut synthetic_id_counter: i64 = -1;
    let mut count = 0;

    let rows: Vec<(i64, String, String, String, f64)> = match stmt.query_map([], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        ))
    }) {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
        Err(_) => return 0,
    };

    for (source_node_id, target_workspace, target_qname, kind, confidence) in rows {
        let src_idx = match id_map.get(&source_node_id) {
            Some(&idx) => idx,
            None => continue,
        };

        // Get or create synthetic node for the external target
        let key = (target_workspace.clone(), target_qname.clone());
        let tgt_idx = *synthetic_nodes.entry(key).or_insert_with(|| {
            let name = target_qname
                .rsplit("::")
                .next()
                .unwrap_or(&target_qname)
                .to_string();
            let idx = graph.add_node(GraphNode {
                db_id: synthetic_id_counter,
                name,
                qualified_name: Some(target_qname.clone()),
                kind: NodeKind::External,
                file_id: -1,
            });
            id_map.insert(synthetic_id_counter, idx);
            synthetic_id_counter -= 1;
            idx
        });

        graph.add_edge(src_idx, tgt_idx, GraphEdge {
            kind: EdgeKind::parse(&kind).unwrap_or(EdgeKind::Calls),
            confidence,
        });
        count += 1;
    }

    count
}
