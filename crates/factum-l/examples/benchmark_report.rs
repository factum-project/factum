use factum_l::*;
use factum_core::types::*;
use factum_core::morphemes::MorphemeRegistry;
use std::sync::Arc;
use factum_l::benchmark::*;

fn main() {
    let registry = Arc::new(MorphemeRegistry::with_seeds());
    let encoder = BaselineEncoder::new(registry.clone());
    let decoder = BaselineDecoder::new(registry.clone());
    let sequiv = SemEquiv::new(registry);

    let nodes = generate_benchmark_nodes();

    // === Ungated benchmark ===
    println!("=== Ungated Benchmark (no anti-cheat gates) ===");
    let result = bench_semantic_roundtrip(&encoder, &decoder, &sequiv, &nodes);
    println!("Total nodes: {}", result.total);
    println!("Pass rate: {:.1}% (threshold: {})", result.passed * 100.0, DEFAULT_THRESHOLD);
    println!("Avg score: {:.4}", result.avg_score);
    println!("Min score: {:.4}", result.min_score);
    println!("Max score: {:.4}", result.max_score);
    println!("v0.1 target: {} | Acceptance: {}", V01_TARGET, ACCEPTANCE_THRESHOLD);
    println!();
    println!("Per-node details:");
    for d in &result.details {
        println!("  {:8} score={:.3} level={:?} diffs={}", d.node_id, d.score, d.level, d.differences.len());
    }

    // === Gated benchmark ===
    println!();
    println!("=== Gated Benchmark (with anti-cheat gates) ===");
    let gated = bench_semantic_roundtrip_gated(&encoder, &decoder, &sequiv, &nodes);
    println!("Total test nodes: {} (train/test split applied)", gated.total);
    println!("Pass rate: {:.1}%", gated.passed * 100.0);
    println!("Avg score: {:.4}", gated.avg_score);
    println!("Min score: {:.4}", gated.min_score);
    println!("Max score: {:.4}", gated.max_score);
    println!();
    println!("Per-node details:");
    for d in &gated.details {
        println!("  {:8} score={:.3} level={:?} diffs={}", d.node_id, d.score, d.level, d.differences.len());
        for diff in &d.differences {
            println!("           - {}", diff);
        }
    }

    // === Dimension budget check ===
    println!();
    println!("=== Dimension Budget Gate ===");
    let gates = AntiCheatGates::default();
    let sample_node = &nodes[0];
    let canonical = factum_core::serialize::canonical(sample_node);
    let enc_result = encoder.encode_node(sample_node).unwrap();
    println!("Sample node: {}", sample_node.id);
    println!("Canonical text: {} bytes", canonical.len());
    println!("Canonical: {}", canonical);
    println!("Vector dim: {}", enc_result.vector.dim);
    println!("Budget (bytes/3): {}", canonical.len() / 3);
    match gates.check_dimension_budget(enc_result.vector.dim, canonical.len()) {
        Ok(()) => println!("Gate 1: PASS"),
        Err(msg) => println!("Gate 1: FAIL — {}", msg),
    }

    // === Argument order test ===
    println!();
    println!("=== Argument Order Preservation (P0-2 fix) ===");
    let node_xy = Node::new("n1", Predicate::new("ceo-of")
        .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]));
    let node_yx = Node::new("n2", Predicate::new("ceo-of")
        .with_args(vec![Term::ent("ACME-CORP"), Term::ent("PERSON-X")]));

    let r_xy = encoder.encode_node(&node_xy).unwrap();
    let r_yx = encoder.encode_node(&node_yx).unwrap();
    let cosine = r_xy.vector.cosine_similarity(&r_yx.vector);
    println!("(ceo-of @PERSON-X @ACME-CORP) vs (ceo-of @ACME-CORP @PERSON-X)");
    println!("Cosine similarity: {:.6}", cosine);
    println!("Vectors differ: {}", r_xy.vector.data != r_yx.vector.data);
    println!();

    // Compare with same-args node
    let node_same = Node::new("n3", Predicate::new("ceo-of")
        .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]));
    let r_same = encoder.encode_node(&node_same).unwrap();
    let cosine_same = r_xy.vector.cosine_similarity(&r_same.vector);
    println!("(ceo-of @PERSON-X @ACME-CORP) vs identical node:");
    println!("Cosine similarity: {:.6}", cosine_same);

    // === SemEquiv demo ===
    println!();
    println!("=== SemEquiv Scoring Demo ===");
    let original = Node::new("orig", Predicate::new("located-in")
        .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]))
        .with_confidence(Confidence(0.9));

    // Exact match
    let exact = Node::new("decoded1", Predicate::new("located-in")
        .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]))
        .with_confidence(Confidence(0.9));
    let r = sequiv.compare(&original, &exact);
    println!("Exact match:        score={:.2} level={:?}", r.score, r.level);

    // Wrong entity
    let wrong_ent = Node::new("decoded2", Predicate::new("located-in")
        .with_args(vec![Term::ent("ACME-CORP"), Term::ent("MOUNTAIN-VIEW")]));
    let r = sequiv.compare(&original, &wrong_ent);
    println!("Wrong entity:       score={:.2} level={:?}", r.score, r.level);

    // Wrong literal
    let wrong_lit = Node::new("decoded3", Predicate::new("revenue")
        .with_args(vec![Term::ent("ACME-CORP"), Term::lit(Literal::dec_from_str("1000.00").unwrap())]));
    let orig_rev = Node::new("orig2", Predicate::new("revenue")
        .with_args(vec![Term::ent("ACME-CORP"), Term::lit(Literal::dec_from_str("2000.00").unwrap())]));
    let r = sequiv.compare(&orig_rev, &wrong_lit);
    println!("Wrong literal:      score={:.2} level={:?}", r.score, r.level);

    // Metadata drift
    let meta_drift = Node::new("decoded4", Predicate::new("located-in")
        .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]))
        .with_confidence(Confidence(0.5));
    let r = sequiv.compare(&original, &meta_drift);
    println!("Metadata drift:     score={:.2} level={:?}", r.score, r.level);

    // Different head
    let diff_head = Node::new("decoded5", Predicate::new("active")
        .with_args(vec![Term::ent("ACME-CORP")]));
    let r = sequiv.compare(&original, &diff_head);
    println!("Different head:     score={:.2} level={:?}", r.score, r.level);
}
