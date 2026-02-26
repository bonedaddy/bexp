use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};

use super::{GraphEdge, GraphNode};

/// Compute PageRank scores for all nodes in the graph.
///
/// Uses the power iteration method with configurable damping factor,
/// maximum iterations, and convergence tolerance.
pub fn compute_pagerank(
    graph: &DiGraph<GraphNode, GraphEdge>,
    damping: f64,
    max_iterations: usize,
    tolerance: f64,
) -> HashMap<NodeIndex, f64> {
    let n = graph.node_count();
    if n == 0 {
        return HashMap::new();
    }

    let n_f64 = n as f64;
    let initial = 1.0 / n_f64;

    let mut scores: HashMap<NodeIndex, f64> =
        graph.node_indices().map(|idx| (idx, initial)).collect();

    // Pre-compute out-degrees for all nodes (avoids recomputing per-neighbor per-iteration)
    let out_degrees: HashMap<NodeIndex, f64> = graph
        .node_indices()
        .map(|idx| {
            let degree = graph
                .neighbors_directed(idx, petgraph::Direction::Outgoing)
                .count() as f64;
            (idx, degree)
        })
        .collect();

    // Collect dangling nodes (no outgoing edges) once
    let dangling_nodes: Vec<NodeIndex> = graph
        .node_indices()
        .filter(|idx| out_degrees[idx] == 0.0)
        .collect();

    let mut converged = false;
    for iteration in 0..max_iterations {
        let mut new_scores: HashMap<NodeIndex, f64> = HashMap::with_capacity(n);
        let mut max_diff = 0.0_f64;

        // Compute dangling node contribution
        let dangling_sum: f64 = dangling_nodes.iter().map(|idx| scores[idx]).sum();

        let dangling_contribution = dangling_sum / n_f64;

        for node in graph.node_indices() {
            let mut sum = 0.0;

            // Sum contributions from incoming neighbors
            for neighbor in graph.neighbors_directed(node, petgraph::Direction::Incoming) {
                let out_degree = out_degrees[&neighbor];
                if out_degree > 0.0 {
                    sum += scores[&neighbor] / out_degree;
                }
            }

            let new_score = (1.0 - damping) / n_f64 + damping * (sum + dangling_contribution);
            let diff = (new_score - scores[&node]).abs();
            max_diff = max_diff.max(diff);
            new_scores.insert(node, new_score);
        }

        scores = new_scores;

        if max_diff < tolerance {
            tracing::debug!("PageRank converged after {} iterations", iteration + 1);
            converged = true;
            break;
        }
    }

    if !converged {
        tracing::warn!(
            max_iterations = max_iterations,
            nodes = n,
            "PageRank did not converge within max iterations"
        );
    }

    scores
}
