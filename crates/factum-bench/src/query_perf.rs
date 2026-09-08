//! Query performance benchmark.
//!
//! Measures lookup latency as a function of node count.

use std::time::Instant;
use factum_core::types::*;
use factum_rt::store::FactumStore;
use factum_rt::query::{Query, QueryOptions};

pub struct QueryPerfResult {
    pub node_count: usize,
    pub query_count: usize,
    pub total_ms: u128,
    pub avg_ms: f64,
}

/// Benchmark query performance with N nodes.
pub fn bench_query(node_count: usize) -> QueryPerfResult {
    let store = FactumStore::with_seeds();

    // Populate with test data
    for i in 0..node_count {
        let ent = if i % 100 == 0 { "ACME-CORP" } else { &format!("E{}", i) };
        let node = Node::new(format!("n{:06}", i),
            Predicate::new("instance-of")
                .with_args(vec![Term::ent(ent), Term::ent("organization")]))
            .with_permissions(PermissionTag::PUBLIC);
        let _ = store.insert(node);
    }

    // Run queries
    let query_count = 1000;
    let start = Instant::now();

    for _i in 0..query_count {
        let q = Query::new(
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("ACME-CORP"), Term::var("type")])
        );
        let _ = store.query(&q, &QueryOptions::default());
    }

    let elapsed = start.elapsed();

    QueryPerfResult {
        node_count,
        query_count,
        total_ms: elapsed.as_millis(),
        avg_ms: elapsed.as_secs_f64() * 1000.0 / query_count as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_perf_1000() {
        let result = bench_query(1000);
        println!("Nodes: {}, Queries: {}, Total: {}ms, Avg: {:.3}ms",
            result.node_count, result.query_count, result.total_ms, result.avg_ms);
        assert!(result.avg_ms < 100.0, "Query should be fast");
    }
}
