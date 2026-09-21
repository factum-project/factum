//! Packed codec benchmark report — compares baseline vs PackedCodec encoder.
//!
//! Run with: `cargo run --example packed_codec_bench -p factum-l`

use factum_l::{
    BaselineEncoder, BaselineDecoder,
    PackedCodecEncoder, PackedCodecDecoder, PACKED_CODEC_DIM,
    AntiCheatGates, bench_semantic_roundtrip, bench_semantic_roundtrip_gated,
    SemEquiv,
};
use factum_core::morphemes::MorphemeRegistry;
use factum_core::serialize;
use std::sync::Arc;

fn main() {
    println!("======================================================");
    println!("  Packed Codec Benchmark Report");
    println!("======================================================");
    println!();

    let registry = Arc::new(MorphemeRegistry::with_seeds());
    let nodes = factum_l::benchmark::generate_benchmark_nodes();

    // --- Dimension comparison ---
    println!("--- Dimension Comparison ---");
    let baseline_enc = BaselineEncoder::new(registry.clone());
    let codec_enc = PackedCodecEncoder::new(registry.clone());

    println!("  Baseline encoder dim:     {}", baseline_enc.dim());
    println!("  Packed codec dim:         {}", PACKED_CODEC_DIM);
    println!();

    // Check dimension budget for each node.
    let gates = AntiCheatGates::default();
    println!("  Gate 1 (dimension budget: vector_bits < canonical_bits/3):");
    for node in &nodes {
        let canonical = serialize::canonical(node);
        let canonical_bytes = canonical.len();
        let budget = canonical_bytes / gates.dimension_budget_divisor;

        let baseline_pass = gates.check_dimension_budget(baseline_enc.dim(), canonical_bytes).is_ok();
        let codec_pass = gates.check_dimension_budget(PACKED_CODEC_DIM, canonical_bytes).is_ok();

        println!(
            "    {}: {} bytes -> budget {}, baseline({}) {} codec({}) {}",
            node.id,
            canonical_bytes,
            budget,
            baseline_enc.dim(),
            if baseline_pass { "PASS" } else { "FAIL" },
            PACKED_CODEC_DIM,
            if codec_pass { "PASS" } else { "FAIL" },
        );
    }
    println!();

    // --- Baseline benchmark (ungated) ---
    println!("--- Baseline Encoder (Ungated) ---");
    let baseline_dec = BaselineDecoder::new(registry.clone());
    let seq = SemEquiv::new(registry.clone());

    let result = bench_semantic_roundtrip(&baseline_enc, &baseline_dec, &seq, &nodes);
    println!("  Total: {}, Passed: {:.0}%", result.total, result.passed * 100.0);
    println!("  Avg score: {:.3}, Min: {:.3}, Max: {:.3}", result.avg_score, result.min_score, result.max_score);
    println!();

    // --- Baseline benchmark (gated) ---
    println!("--- Baseline Encoder (Gated) ---");
    let result = bench_semantic_roundtrip_gated(&baseline_enc, &baseline_dec, &seq, &nodes);
    println!("  Total: {}, Passed: {:.0}%", result.total, result.passed * 100.0);
    println!("  Avg score: {:.3}, Min: {:.3}, Max: {:.3}", result.avg_score, result.min_score, result.max_score);
    for detail in &result.details {
        println!("    {}: score={:.3} level={:?} diffs={:?}",
            detail.node_id, detail.score, detail.level, detail.differences);
    }
    println!();

    // --- Packed codec benchmark (ungated) ---
    println!("--- Packed Codec Encoder (Ungated) ---");
    let codec_dec = PackedCodecDecoder::new(registry.clone());

    let result = bench_semantic_roundtrip(&codec_enc, &codec_dec, &seq, &nodes);
    println!("  Total: {}, Passed: {:.0}%", result.total, result.passed * 100.0);
    println!("  Avg score: {:.3}, Min: {:.3}, Max: {:.3}", result.avg_score, result.min_score, result.max_score);
    for detail in &result.details {
        println!("    {}: score={:.3} level={:?}",
            detail.node_id, detail.score, detail.level);
    }
    println!();

    // --- Packed codec benchmark (gated) ---
    println!("--- Packed Codec Encoder (Gated) ---");
    let result = bench_semantic_roundtrip_gated(&codec_enc, &codec_dec, &seq, &nodes);
    println!("  Total: {}, Passed: {:.0}%", result.total, result.passed * 100.0);
    println!("  Avg score: {:.3}, Min: {:.3}, Max: {:.3}", result.avg_score, result.min_score, result.max_score);
    for detail in &result.details {
        println!("    {}: score={:.3} level={:?} diffs={:?}",
            detail.node_id, detail.score, detail.level, detail.differences);
    }
    println!();

    // --- Summary ---
    println!("--- Summary ---");
    println!("  Baseline:    {} dims, avg score {:.3} (gated)", baseline_enc.dim(), bench_semantic_roundtrip_gated(&baseline_enc, &baseline_dec, &seq, &nodes).avg_score);
    println!("  PackedCodec: {} dims, avg score {:.3} (gated)", PACKED_CODEC_DIM, bench_semantic_roundtrip_gated(&codec_enc, &codec_dec, &seq, &nodes).avg_score);
    println!();
    println!("  Improvement: {:.1}x dimension reduction, {:.1}x score improvement",
        baseline_enc.dim() as f32 / PACKED_CODEC_DIM as f32,
        bench_semantic_roundtrip_gated(&codec_enc, &codec_dec, &seq, &nodes).avg_score
            / bench_semantic_roundtrip_gated(&baseline_enc, &baseline_dec, &seq, &nodes).avg_score.max(0.001),
    );
}
