use std::time::Duration;

use metrics::{counter, gauge, histogram};

pub fn record_tool_call(tool_name: &str) {
    counter!("bexp_mcp_tool_calls_total", "tool" => tool_name.to_string()).increment(1);
}

pub fn record_tool_duration(tool_name: &str, duration: Duration) {
    histogram!("bexp_mcp_tool_duration_seconds", "tool" => tool_name.to_string())
        .record(duration.as_secs_f64());
}

pub fn record_tool_error(tool_name: &str) {
    counter!("bexp_mcp_tool_errors_total", "tool" => tool_name.to_string()).increment(1);
}

pub fn record_index_complete(files: usize, nodes: usize, edges: usize) {
    gauge!("bexp_index_files").set(files as f64);
    gauge!("bexp_index_nodes").set(nodes as f64);
    gauge!("bexp_index_edges").set(edges as f64);
}

pub fn set_index_ready(ready: bool) {
    gauge!("bexp_index_ready").set(if ready { 1.0 } else { 0.0 });
}

pub fn record_cache_hit() {
    counter!("bexp_capsule_cache_hits_total").increment(1);
}

pub fn record_cache_miss() {
    counter!("bexp_capsule_cache_misses_total").increment(1);
}

#[allow(dead_code)]
pub fn record_db_operation(op_type: &str) {
    counter!("bexp_db_operations_total", "op" => op_type.to_string()).increment(1);
}

pub fn record_observation_saved() {
    counter!("bexp_observations_saved_total").increment(1);
}

pub fn set_graph_stats(nodes: usize, edges: usize) {
    gauge!("bexp_graph_nodes").set(nodes as f64);
    gauge!("bexp_graph_edges").set(edges as f64);
}
