//! Syntax round-trip benchmark.
//!
//! Verifies that parse(serialize(parse(x))) == parse(x) for all nodes.
//! This MUST be 100% — it's the trust foundation of the system.

use factum_core::serialize;
use factum_core::types::*;

pub struct RoundTripResult {
    pub total: usize,
    pub passed: usize,
    pub success_rate: f64,
    pub failures: Vec<String>,
}

/// Benchmark round-trip fidelity with generated nodes.
pub fn bench_roundtrip(count: usize) -> RoundTripResult {
    let nodes = generate_test_nodes(count);
    let mut passed = 0;
    let mut failures = Vec::new();

    for node in &nodes {
        match serialize::verify_roundtrip(node) {
            Ok(()) => passed += 1,
            Err(e) => failures.push(format!("Node {}: {}", node.id, e)),
        }
    }

    RoundTripResult {
        total: count,
        passed,
        success_rate: passed as f64 / count as f64,
        failures,
    }
}

/// Generate a variety of test nodes for benchmarking.
fn generate_test_nodes(count: usize) -> Vec<Node> {
    let mut nodes = Vec::with_capacity(count);
    let entities = ["ACME-CORP", "APPLE", "GOOGLE", "MICROSOFT", "TESLA", "AMAZON"];
    let relations = ["instance-of", "located-in", "founded-on", "ceo-of", "revenue"];
    let locations = ["ACME-HQ", "CUPERTINO", "MOUNTAIN-VIEW", "REDMOND", "AUSTIN", "SEATTLE"];

    for i in 0..count {
        let ent_idx = i % entities.len();
        let rel_idx = i % relations.len();

        let pred = match rel_idx {
            0 => Predicate::new("instance-of")
                .with_args(vec![Term::ent(entities[ent_idx]), Term::ent("organization")]),
            1 => Predicate::new("located-in")
                .with_args(vec![Term::ent(entities[ent_idx]), Term::ent(locations[ent_idx])]),
            2 => Predicate::new("founded-on")
                .with_args(vec![
                    Term::ent(entities[ent_idx]),
                    Term::lit(Literal::Date(chrono::NaiveDate::from_ymd_opt(1976 + (i % 30) as i32, 4, 1).unwrap())),
                ]),
            3 => Predicate::new("ceo-of")
                .with_args(vec![Term::ent("PERSON-X"), Term::ent(entities[ent_idx])]),
            _ => Predicate::new("revenue")
                .with_args(vec![
                    Term::ent(entities[ent_idx]),
                    Term::lit(Literal::dec_from_str(&format!("{}.00", (i + 1) * 1000000)).unwrap()),
                ]),
        };

        let node = Node::new(format!("n{:04}", i), pred)
            .with_confidence(Confidence(0.5 + (i % 5) as f32 * 0.1))
            .with_authority(Authority(0.6 + (i % 4) as f32 * 0.1));

        nodes.push(node);
    }

    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_100() {
        let result = bench_roundtrip(100);
        assert_eq!(result.success_rate, 1.0, "Round-trip must be 100%");
    }

    #[test]
    fn test_roundtrip_1000() {
        let result = bench_roundtrip(1000);
        assert_eq!(result.success_rate, 1.0, "Round-trip must be 100%");
        if !result.failures.is_empty() {
            panic!("Failures: {:?}", &result.failures[..5.min(result.failures.len())]);
        }
    }
}
