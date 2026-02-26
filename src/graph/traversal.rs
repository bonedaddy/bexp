use std::collections::HashSet;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;

use super::{GraphEdge, GraphNode};

/// BFS to find all callers (incoming edges) up to `depth` levels.
pub fn get_callers(
    graph: &DiGraph<GraphNode, GraphEdge>,
    start: NodeIndex,
    depth: usize,
    edge_kinds: Option<&[String]>,
) -> String {
    let mut output = format!("# Callers of `{}`\n\n", graph[start].name);
    let visited = bfs_direction(graph, start, depth, Direction::Incoming, edge_kinds);

    for (level, nodes) in visited.iter().enumerate() {
        if nodes.is_empty() {
            continue;
        }
        output.push_str(&format!("## Depth {level}\n\n"));
        for &idx in nodes {
            let node = &graph[idx];
            output.push_str(&format!(
                "- `{}` ({})\n",
                node.qualified_name.as_deref().unwrap_or(&node.name),
                node.kind
            ));
        }
        output.push('\n');
    }

    output
}

/// BFS to find all callees (outgoing edges) up to `depth` levels.
pub fn get_callees(
    graph: &DiGraph<GraphNode, GraphEdge>,
    start: NodeIndex,
    depth: usize,
    edge_kinds: Option<&[String]>,
) -> String {
    let mut output = format!("# Callees of `{}`\n\n", graph[start].name);
    let visited = bfs_direction(graph, start, depth, Direction::Outgoing, edge_kinds);

    for (level, nodes) in visited.iter().enumerate() {
        if nodes.is_empty() {
            continue;
        }
        output.push_str(&format!("## Depth {level}\n\n"));
        for &idx in nodes {
            let node = &graph[idx];
            output.push_str(&format!(
                "- `{}` ({})\n",
                node.qualified_name.as_deref().unwrap_or(&node.name),
                node.kind
            ));
        }
        output.push('\n');
    }

    output
}

fn bfs_direction(
    graph: &DiGraph<GraphNode, GraphEdge>,
    start: NodeIndex,
    max_depth: usize,
    direction: Direction,
    edge_kinds: Option<&[String]>,
) -> Vec<Vec<NodeIndex>> {
    let mut levels: Vec<Vec<NodeIndex>> = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(start);

    let mut current_level = vec![start];

    for _ in 0..max_depth {
        let mut next_level = Vec::new();
        for &node in &current_level {
            let mut edges = graph.neighbors_directed(node, direction).detach();
            while let Some((edge_idx, neighbor)) = edges.next(graph) {
                if let Some(kinds) = edge_kinds {
                    let edge = &graph[edge_idx];
                    if !kinds.iter().any(|k| k == edge.kind.as_str()) {
                        continue;
                    }
                }
                if visited.insert(neighbor) {
                    next_level.push(neighbor);
                }
            }
        }
        if next_level.is_empty() {
            break;
        }
        levels.push(next_level.clone());
        current_level = next_level;
    }

    levels
}

/// Find all simple paths between two nodes using DFS with backtracking.
pub fn find_all_paths(
    graph: &DiGraph<GraphNode, GraphEdge>,
    from: NodeIndex,
    to: NodeIndex,
    max_depth: usize,
) -> Vec<Vec<NodeIndex>> {
    let mut paths = Vec::new();
    let mut current_path = vec![from];
    let mut visited = HashSet::new();
    visited.insert(from);

    dfs_paths(
        graph,
        from,
        to,
        max_depth,
        &mut current_path,
        &mut visited,
        &mut paths,
    );

    paths
}

fn dfs_paths(
    graph: &DiGraph<GraphNode, GraphEdge>,
    current: NodeIndex,
    target: NodeIndex,
    max_depth: usize,
    path: &mut Vec<NodeIndex>,
    visited: &mut HashSet<NodeIndex>,
    paths: &mut Vec<Vec<NodeIndex>>,
) {
    if current == target {
        paths.push(path.clone());
        return;
    }

    if path.len() > max_depth {
        return;
    }

    // Limit total paths found
    if paths.len() >= 10 {
        return;
    }

    for neighbor in graph.neighbors_directed(current, Direction::Outgoing) {
        if visited.insert(neighbor) {
            path.push(neighbor);
            dfs_paths(graph, neighbor, target, max_depth, path, visited, paths);
            path.pop();
            visited.remove(&neighbor);
        }
    }
}
