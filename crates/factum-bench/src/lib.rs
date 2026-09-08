//! # factum-bench
//!
//! Benchmarks for Factum:
//! 1. **Syntax round-trip**: parse∘serialize identity (MUST be 100%)
//! 2. **Token efficiency**: Factum-F vs Markdown/JSON byte count
//! 3. **Query performance**: lookup latency vs node count
//! 4. **Retraction propagation**: cascade latency
//!
//! ## Principles
//! - All benchmark data is public and reproducible
//! - Results include dimensions where Factum performs poorly
//! - This is the credibility foundation for releases

pub mod round_trip;
pub mod token_efficiency;
pub mod query_perf;

/// Run all benchmarks and print results.
pub fn run_all() {
    println!("=== Factum Benchmarks ===\n");

    print!("1. Syntax round-trip... ");
    let rt = round_trip::bench_roundtrip(1000);
    println!("{:.1}% (target: 100%)", rt.success_rate * 100.0);

    print!("2. Token efficiency... ");
    let te = token_efficiency::bench_efficiency();
    println!("Factum: {} bytes, Markdown: {} bytes, JSON: {} bytes (savings: {:.1}%)",
        te.factum_bytes, te.markdown_bytes, te.json_bytes, te.factum_savings_pct);

    print!("3. Query performance... ");
    let qp = query_perf::bench_query(10000);
    println!("{} queries in {}ms (avg: {:.3}ms)",
        qp.query_count, qp.total_ms, qp.avg_ms);
}
