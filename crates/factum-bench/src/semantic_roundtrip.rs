//! Semantic round-trip benchmark (factum-l integration).
//!
//! This module wraps factum-l's semantic round-trip benchmark for use
//! in factum-bench's `run_all()`. It measures the encode → decode → compare
//! pipeline using the baseline encoder/decoder.

use factum_core::types::*;
use factum_l::{SemEquiv, BaselineEncoder, BaselineDecoder};
use factum_l::benchmark::bench_semantic_roundtrip;
use factum_l::benchmark::SemanticRoundTripResult;
use std::sync::Arc;
use factum_core::morphemes::MorphemeRegistry;

/// Run the semantic round-trip benchmark and return the result.
pub fn bench_semantic() -> SemanticRoundTripResult {
    let registry = Arc::new(MorphemeRegistry::with_seeds());
    let encoder = BaselineEncoder::new(registry.clone());
    let decoder = BaselineDecoder::new(registry.clone());
    let sequiv = SemEquiv::new(registry);

    let nodes = generate_test_nodes();
    bench_semantic_roundtrip(&encoder, &decoder, &sequiv, &nodes)
}

/// Generate diverse test nodes for semantic round-trip benchmarking.
fn generate_test_nodes() -> Vec<Node> {
    vec![
        Node::new("sb001", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")])),

        Node::new("sb002", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ])),

        Node::new("sb003", Predicate::new("shareholder-major")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::ent("FOUNDER-1"),
                Term::lit(Literal::dec_from_str("0.73").unwrap()),
            ])),

        Node::new("sb004", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")])),

        Node::new("sb005", Predicate::new("instance-of")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("organization")])),

        Node::new("sb006", Predicate::new("founded-on")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::Date(chrono::NaiveDate::from_ymd_opt(1976, 4, 1).unwrap())),
            ])),

        Node::new("sb007", Predicate::new("located-in")
            .with_args(vec![Term::ent("APPLE"), Term::ent("CUPERTINO")]))
            .with_confidence(Confidence(0.95))
            .with_authority(Authority(0.9)),

        Node::new("sb008", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("GOOGLE"),
                Term::lit(Literal::dec_from_str("300000000.00").unwrap()),
            ]))
            .with_provenance(Provenance::Asserted {
                by: Principal(smol_str::SmolStr::new("analyst-1")),
            }),

        Node::new("sb009", Predicate::new("subsidiary-of")
            .with_args(vec![Term::ent("SUB-CORP"), Term::ent("ACME-CORP")]))
            .with_dep(NodeId::new("sb001")),

        Node::new("sb010", Predicate::new("active")
            .with_args(vec![Term::ent("ACME-CORP")])),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bench_semantic_runs() {
        let result = bench_semantic();
        assert!(result.total > 0);
        assert!(result.avg_score >= 0.0);
        assert!(result.max_score <= 1.0);
        assert_eq!(result.failures.len(), 0);
    }

    #[test]
    fn test_bench_semantic_head_preserved() {
        // The baseline encoder/decoder preserves predicate heads, but
        // loses entity IDs, literal values, and sometimes arg count
        // (type features are normalized and may round incorrectly).
        // With v2 calibrated scoring:
        // - Wrong entities → WrongValues (≤0.3)
        // - Arg count mismatch → Different (0.0)
        // So we expect most nodes to score in 0.0–0.3, with some at 0.0
        // due to arg count reconstruction failures.
        let result = bench_semantic();
        for detail in &result.details {
            assert!(
                detail.score <= 0.3,
                "Node {} scored {} — expected <= 0.3 (head preserved but values lost). Level: {:?}, Diffs: {:?}",
                detail.node_id, detail.score, detail.level, detail.differences
            );
        }
    }
}
