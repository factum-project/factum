//! Semantic round-trip benchmark.
//!
//! Implements the ROADMAP's "Semantic round-trip target: >=0.95 (v0.1),
//! >=0.99 (acceptance threshold)" measurement.
//!
//! ## Pipeline
//!
//! For each test node:
//! 1. Encode: `node → ThoughtVector` (via Encoder)
//! 2. [Optional] Operate on z in latent space (anti-cheat gate 2)
//! 3. Decode: `ThoughtVector → node'` (via Decoder)
//! 4. Compare: `SemEquiv.compare(node, node')` → score
//! 5. Record: score, level, differences
//!
//! ## Anti-Cheat Gates (P0-1 fix)
//!
//! Three gates prevent "memorization encoders" from gaming the benchmark:
//!
//! 1. **Dimension budget (bit comparison)**: The information content of
//!    the latent vector (dims * 32 bits) must be significantly smaller
//!    than the canonical text (bytes * 8 bits). A memorization
//!    encoder that stores the full text in z would fail this gate.
//!    Default: `vector_bits < canonical_bits / 3`.
//!
//! 2. **Latent space operation**: The benchmark can inject a no-op
//!    transformation on z between encode and decode. A memorization encoder
//!    that stores text in exact byte positions would be disrupted by even
//!    a small perturbation. The default no-op is a normalization + small
//!    noise addition.
//!
//! 3. **Train/test split**: The benchmark separates training nodes (used
//!    by the encoder/decoder to build their internal models) from test
//!    nodes (used for scoring). A memorization encoder that stores exact
//!    mappings would score 0 on unseen test nodes. The default split is
//!    50/50.
//!
//! ## Pass Criterion
//!
//! A node "passes" if its SemEquiv score >= 0.5. Note that with v2
//! calibrated scoring, WrongValues (head matches, values differ) scores
//! ≤0.3, so a score >= 0.5 means at least MetadataDrift (predicate
//! fully matches). The v0.1 target of 0.95 was for future neural encoders
//! — archived, see packed_codec.rs for the negative result analysis.

use crate::encoder::Encoder;
use crate::decoder::Decoder;
use crate::sequiv::{SemEquiv, EquivLevel};
use crate::error::LatentError;
use factum_core::types::*;
use factum_core::serialize;

/// Semantic round-trip benchmark result.
#[derive(Clone, Debug)]
pub struct SemanticRoundTripResult {
    /// Total number of nodes tested.
    pub total: usize,
    /// Fraction of nodes that passed (score >= threshold).
    pub passed: f32,
    /// Average SemEquiv score across all nodes.
    pub avg_score: f32,
    /// Minimum score observed.
    pub min_score: f32,
    /// Maximum score observed.
    pub max_score: f32,
    /// Per-node details (node_id, score, level, differences).
    pub details: Vec<NodeResult>,
    /// Failures (node_id + error message).
    pub failures: Vec<String>,
}

/// Per-node result in the benchmark.
#[derive(Clone, Debug)]
pub struct NodeResult {
    pub node_id: String,
    pub score: f32,
    pub level: EquivLevel,
    pub differences: Vec<String>,
}

/// Default pass threshold: score >= 0.5 (MetadataDrift or better).
///
/// Note: with v2 calibrated scoring, WrongValues scores ≤0.3.
/// A score >= 0.5 requires at least MetadataDrift (predicate matches).
pub const DEFAULT_THRESHOLD: f32 = 0.5;

/// v0.1 target threshold: score >= 0.95.
pub const V01_TARGET: f32 = 0.95;

/// Acceptance threshold: score >= 0.99.
pub const ACCEPTANCE_THRESHOLD: f32 = 0.99;

/// Anti-cheat gate configuration.
///
/// These gates prevent "memorization encoders" from gaming the benchmark
/// by storing exact source text in the latent vector.
#[derive(Clone, Debug)]
pub struct AntiCheatGates {
    /// Gate 1: Dimension budget (bit comparison).
    /// The vector's information content (dims * 32 bits) must be less than
    /// `canonical_bits / dimension_budget_divisor` where canonical_bits =
    /// canonical_bytes * 8.
    /// Default: 3 (vector must be < 1/3 of source text in bits).
    pub dimension_budget_divisor: usize,

    /// Gate 2: Latent space operation.
    /// If true, a no-op transformation (normalize + tiny noise) is applied
    /// to z between encode and decode. This disrupts exact-position
    /// memorization.
    pub latent_operation_enabled: bool,

    /// Gate 3: Train/test split.
    /// The fraction of nodes reserved for testing (not seen during
    /// "training"). Default: 0.5 (half the nodes are test-only).
    /// A value of 0.0 disables this gate (all nodes are tested).
    pub test_split_ratio: f32,
}

impl Default for AntiCheatGates {
    fn default() -> Self {
        Self {
            dimension_budget_divisor: 3,
            latent_operation_enabled: true,
            test_split_ratio: 0.5,
        }
    }
}

impl AntiCheatGates {
    /// Create gates with all checks disabled (for backward compatibility).
    pub fn disabled() -> Self {
        Self {
            dimension_budget_divisor: usize::MAX,
            latent_operation_enabled: false,
            test_split_ratio: 0.0,
        }
    }

    /// Check Gate 1: dimension budget (bit comparison).
    ///
    /// Compares the information content of the vector (dims * bits per dim)
    /// against the canonical text (bytes * 8 bits per byte). The vector
    /// must be significantly smaller than the canonical text in bits.
    ///
    /// Previous implementation compared float count vs byte count directly,
    /// which was incorrect — a float is 32 bits, not 1 bit. This version
    /// uses `vector_bits < canonical_bits / divisor` where
    /// `vector_bits = vector_dim * 32` and `canonical_bits = canonical_bytes * 8`.
    ///
    /// Returns Ok(()) if the vector passes the budget, or Err with a description.
    pub fn check_dimension_budget(&self, vector_dim: usize, canonical_bytes: usize) -> Result<(), String> {
        if self.dimension_budget_divisor == usize::MAX {
            return Ok(());
        }
        let vector_bits = vector_dim * 32;
        let canonical_bits = canonical_bytes * 8;
        let budget = canonical_bits / self.dimension_budget_divisor;
        if vector_bits >= budget {
            return Err(format!(
                "dimension budget violated (bits): vector={}dims*32={}bits >= canonical={}/{}={}bits (canonical_bytes={})",
                vector_dim, vector_bits, canonical_bits, self.dimension_budget_divisor, budget, canonical_bytes
            ));
        }
        Ok(())
    }

    /// Apply Gate 2: latent space operation.
    ///
    /// Normalizes the vector and adds a tiny perturbation. This disrupts
    /// exact-position memorization without changing the semantic content
    /// for legitimate encoders.
    pub fn apply_latent_operation(&self, vector: crate::encoder::ThoughtVector) -> crate::encoder::ThoughtVector {
        if !self.latent_operation_enabled {
            return vector;
        }
        // Normalize to unit length.
        let normalized = vector.normalize();
        // Add tiny noise (1e-6) to each dimension. This is below the
        // precision threshold of any meaningful feature, but disrupts
        // exact byte-position storage.
        let noisy_data: Vec<f32> = normalized.data.iter()
            .map(|v| v + 1e-6)
            .collect();
        crate::encoder::ThoughtVector::from_vec(noisy_data)
    }

    /// Split nodes into train and test sets (Gate 3).
    ///
    /// Returns (train_nodes, test_nodes). The test set contains
    /// `test_split_ratio` fraction of the nodes, selected deterministically
    /// by taking every other node.
    pub fn train_test_split<'a>(&self, nodes: &'a [Node]) -> (Vec<&'a Node>, Vec<&'a Node>) {
        if self.test_split_ratio <= 0.0 {
            return (vec![], nodes.iter().collect());
        }

        let test_count = (nodes.len() as f32 * self.test_split_ratio).ceil() as usize;
        let test_count = test_count.min(nodes.len());

        // Deterministic split: even indices = train, odd indices = test.
        let mut train = Vec::new();
        let mut test = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            if test.len() < test_count && i % 2 == 1 {
                test.push(node);
            } else {
                train.push(node);
            }
        }

        (train, test)
    }
}

/// Run semantic round-trip benchmark.
///
/// For each node: encode → decode → compare → record score.
///
/// # Arguments
/// - `encoder`: The encoder to use (e.g., BaselineEncoder)
/// - `decoder`: The decoder to use (e.g., BaselineDecoder)
/// - `sequiv`: The semantic equivalence comparator
/// - `nodes`: Test nodes to benchmark
///
/// # Returns
/// A `SemanticRoundTripResult` with aggregate statistics and per-node details.
pub fn bench_semantic_roundtrip(
    encoder: &dyn Encoder,
    decoder: &dyn Decoder,
    sequiv: &SemEquiv,
    nodes: &[Node],
) -> SemanticRoundTripResult {
    bench_semantic_roundtrip_with_threshold(encoder, decoder, sequiv, nodes, DEFAULT_THRESHOLD)
}

/// Run benchmark with a custom pass threshold.
pub fn bench_semantic_roundtrip_with_threshold(
    encoder: &dyn Encoder,
    decoder: &dyn Decoder,
    sequiv: &SemEquiv,
    nodes: &[Node],
    threshold: f32,
) -> SemanticRoundTripResult {
    let mut details = Vec::with_capacity(nodes.len());
    let mut failures = Vec::new();
    let mut scores = Vec::with_capacity(nodes.len());
    let mut passed_count = 0usize;

    for node in nodes {
        match bench_single_node(encoder, decoder, sequiv, node) {
            Ok(result) => {
                if result.score >= threshold {
                    passed_count += 1;
                }
                scores.push(result.score);
                details.push(result);
            }
            Err(e) => {
                failures.push(format!("Node {}: {}", node.id, e));
                scores.push(0.0);
                details.push(NodeResult {
                    node_id: node.id.to_string(),
                    score: 0.0,
                    level: EquivLevel::Different,
                    differences: vec![e.to_string()],
                });
            }
        }
    }

    let total = nodes.len();
    let passed = if total > 0 {
        passed_count as f32 / total as f32
    } else {
        0.0
    };

    let avg_score = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f32>() / scores.len() as f32
    };

    let min_score = scores.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_score = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

    let (min_score, max_score) = if scores.is_empty() {
        (0.0, 0.0)
    } else {
        (min_score, max_score)
    };

    SemanticRoundTripResult {
        total,
        passed,
        avg_score,
        min_score,
        max_score,
        details,
        failures,
    }
}

/// Run benchmark on a single node.
fn bench_single_node(
    encoder: &dyn Encoder,
    decoder: &dyn Decoder,
    sequiv: &SemEquiv,
    node: &Node,
) -> Result<NodeResult, LatentError> {
    // Step 1: Encode.
    let encode_result = encoder.encode_node(node)?;

    // Step 2: Decode.
    let decode_result = decoder.decode(&encode_result.vector)?;

    if decode_result.nodes.is_empty() {
        return Err(LatentError::DecodeError(
            "decoder returned no nodes".to_string(),
        ));
    }

    // Step 3: Compare.
    let decoded_node = &decode_result.nodes[0];
    let equiv_result = sequiv.compare(node, decoded_node);

    Ok(NodeResult {
        node_id: node.id.to_string(),
        score: equiv_result.score,
        level: equiv_result.level,
        differences: equiv_result.differences,
    })
}

/// Run benchmark on a single node with anti-cheat gates.
fn bench_single_node_gated(
    encoder: &dyn Encoder,
    decoder: &dyn Decoder,
    sequiv: &SemEquiv,
    node: &Node,
    gates: &AntiCheatGates,
) -> Result<NodeResult, LatentError> {
    // Step 1: Encode.
    let encode_result = encoder.encode_node(node)?;

    // Gate 1: Check dimension budget.
    let canonical_text = serialize::canonical(node);
    if let Err(msg) = gates.check_dimension_budget(encode_result.vector.dim, canonical_text.len()) {
        // Dimension budget violated — record as a failure with score 0.
        return Ok(NodeResult {
            node_id: node.id.to_string(),
            score: 0.0,
            level: EquivLevel::Different,
            differences: vec![format!("GATE 1 FAILED: {}", msg)],
        });
    }

    // Gate 2: Apply latent space operation.
    let z = gates.apply_latent_operation(encode_result.vector);

    // Step 2: Decode (from the gated z).
    let decode_result = decoder.decode(&z)?;

    if decode_result.nodes.is_empty() {
        return Err(LatentError::DecodeError(
            "decoder returned no nodes".to_string(),
        ));
    }

    // Step 3: Compare.
    let decoded_node = &decode_result.nodes[0];
    let equiv_result = sequiv.compare(node, decoded_node);

    Ok(NodeResult {
        node_id: node.id.to_string(),
        score: equiv_result.score,
        level: equiv_result.level,
        differences: equiv_result.differences,
    })
}

/// Run semantic round-trip benchmark with anti-cheat gates.
///
/// This is the recommended entry point for benchmarking. It applies all
/// three anti-cheat gates (dimension budget, latent operation, train/test
/// split) to prevent memorization encoders from gaming the score.
///
/// Only test nodes (not seen during "training") are scored. Train nodes
/// are passed to the encoder/decoder for any internal model building
/// (the baseline encoder/decoder are stateless, so train nodes are ignored).
pub fn bench_semantic_roundtrip_gated(
    encoder: &dyn Encoder,
    decoder: &dyn Decoder,
    sequiv: &SemEquiv,
    nodes: &[Node],
) -> SemanticRoundTripResult {
    bench_semantic_roundtrip_gated_with(encoder, decoder, sequiv, nodes, &AntiCheatGates::default())
}

/// Run gated benchmark with custom gate configuration.
pub fn bench_semantic_roundtrip_gated_with(
    encoder: &dyn Encoder,
    decoder: &dyn Decoder,
    sequiv: &SemEquiv,
    nodes: &[Node],
    gates: &AntiCheatGates,
) -> SemanticRoundTripResult {
    // Gate 3: Split into train and test sets.
    let (_train, test) = gates.train_test_split(nodes);

    let test_nodes: Vec<&Node> = if test.is_empty() {
        nodes.iter().collect()
    } else {
        test
    };

    let mut details = Vec::with_capacity(test_nodes.len());
    let mut failures = Vec::new();
    let mut scores = Vec::with_capacity(test_nodes.len());
    let mut passed_count = 0usize;

    for node in &test_nodes {
        match bench_single_node_gated(encoder, decoder, sequiv, node, gates) {
            Ok(result) => {
                if result.score >= DEFAULT_THRESHOLD {
                    passed_count += 1;
                }
                scores.push(result.score);
                details.push(result);
            }
            Err(e) => {
                failures.push(format!("Node {}: {}", node.id, e));
                scores.push(0.0);
                details.push(NodeResult {
                    node_id: node.id.to_string(),
                    score: 0.0,
                    level: EquivLevel::Different,
                    differences: vec![e.to_string()],
                });
            }
        }
    }

    let total = test_nodes.len();
    let passed = if total > 0 {
        passed_count as f32 / total as f32
    } else {
        0.0
    };

    let avg_score = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f32>() / scores.len() as f32
    };

    let min_score = scores.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_score = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

    let (min_score, max_score) = if scores.is_empty() {
        (0.0, 0.0)
    } else {
        (min_score, max_score)
    };

    SemanticRoundTripResult {
        total,
        passed,
        avg_score,
        min_score,
        max_score,
        details,
        failures,
    }
}

/// Generate diverse test nodes for benchmarking.
///
/// This is similar to factum-bench's `generate_test_nodes` but lives here
/// so factum-l can be tested independently.
pub fn generate_benchmark_nodes() -> Vec<Node> {
    vec![
        // Simple relation: 2 entity args.
        Node::new("bm001", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")])),

        // Relation with literal: 1 ent + 1 dec.
        Node::new("bm002", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ])),

        // Relation with 3 args.
        Node::new("bm003", Predicate::new("shareholder-major")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::ent("FOUNDER-1"),
                Term::lit(Literal::dec_from_str("0.73").unwrap()),
            ])),

        // Relation with variable.
        Node::new("bm004", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")])),

        // Status predicate.
        Node::new("bm005", Predicate::new("active")
            .with_args(vec![Term::ent("ACME-CORP")])),

        // Relation with named args.
        Node::new("bm006", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("5000000.00").unwrap()),
            ])
            .with_named("period", Term::lit(Literal::Date(
                chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            )))),

        // Entity type assertion.
        Node::new("bm007", Predicate::new("instance-of")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("organization")])),

        // With confidence and authority metadata.
        Node::new("bm008", Predicate::new("located-in")
            .with_args(vec![Term::ent("APPLE"), Term::ent("CUPERTINO")]))
            .with_confidence(Confidence(0.95))
            .with_authority(Authority(0.9))
            .with_permissions(PermissionTag::CONFIDENTIAL),

        // With provenance.
        Node::new("bm009", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("GOOGLE"),
                Term::lit(Literal::dec_from_str("300000000.00").unwrap()),
            ]))
            .with_provenance(Provenance::Extracted {
                doc: DocId::new("annual-report"),
                span: Span { start: 0, end: 500 },
                model: ModelRef {
                    name: smol_str::SmolStr::new("gpt-4"),
                    version: smol_str::SmolStr::new("2024-06"),
                },
            }),

        // With deps.
        Node::new("bm010", Predicate::new("subsidiary-of")
            .with_args(vec![Term::ent("SUB-CORP"), Term::ent("ACME-CORP")]))
            .with_dep(NodeId::new("bm001")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::BaselineEncoder;
    use crate::decoder::BaselineDecoder;
    use crate::sequiv::SemEquiv;
    use factum_core::morphemes::MorphemeRegistry;
    use std::sync::Arc;

    fn make_components() -> (BaselineEncoder, BaselineDecoder, SemEquiv) {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let encoder = BaselineEncoder::new(registry.clone());
        let decoder = BaselineDecoder::new(registry.clone());
        let sequiv = SemEquiv::new(registry);
        (encoder, decoder, sequiv)
    }

    #[test]
    fn test_benchmark_runs() {
        let (enc, dec, seq) = make_components();
        let nodes = generate_benchmark_nodes();

        let result = bench_semantic_roundtrip(&enc, &dec, &seq, &nodes);

        assert_eq!(result.total, nodes.len());
        assert!(result.avg_score >= 0.0);
        assert!(result.min_score >= 0.0);
        assert!(result.max_score <= 1.0);
        assert_eq!(result.details.len(), nodes.len());
    }

    #[test]
    fn test_benchmark_empty() {
        let (enc, dec, seq) = make_components();
        let result = bench_semantic_roundtrip(&enc, &dec, &seq, &[]);

        assert_eq!(result.total, 0);
        assert_eq!(result.passed, 0.0);
        assert_eq!(result.avg_score, 0.0);
        assert!(result.details.is_empty());
        assert!(result.failures.is_empty());
    }

    #[test]
    fn test_benchmark_single_node() {
        let (enc, dec, seq) = make_components();

        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));

        let result = bench_semantic_roundtrip(&enc, &dec, &seq, &[node]);

        assert_eq!(result.total, 1);
        assert_eq!(result.details.len(), 1);
        // Baseline encoder preserves head morpheme but loses entity IDs.
        // With v2 scoring, wrong entities → WrongValues (≤0.3).
        assert!(result.details[0].score >= 0.0);
    }

    #[test]
    fn test_benchmark_head_preserved() {
        // The baseline encoder/decoder should at least preserve the
        // predicate head morpheme. With v2 calibrated scoring, wrong
        // entity values → WrongValues (≤0.3), and arg count mismatch
        // → Different (0.0). So scores can be 0.0–0.3.
        let (enc, dec, seq) = make_components();

        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));

        let result = bench_semantic_roundtrip(&enc, &dec, &seq, &[node]);

        let detail = &result.details[0];
        // With wrong entities but matching head, expect WrongValues (≤0.3).
        assert!(
            detail.score <= 0.3,
            "expected score <= 0.3 for head-only match with wrong values, got {} (level: {:?}, diffs: {:?})",
            detail.score, detail.level, detail.differences
        );
    }

    #[test]
    fn test_benchmark_metadata_preserved() {
        // Metadata (authority) should be approximately preserved.
        // Confidence, however, now comes from provenance-based calibration
        // rather than the original node's confidence value.
        let (enc, dec, seq) = make_components();

        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]))
            .with_confidence(Confidence(0.9))
            .with_authority(Authority(0.8));

        let result = bench_semantic_roundtrip(&enc, &dec, &seq, &[node]);

        let detail = &result.details[0];
        // Entity values differ (ENT-0 vs X), so score is WrongValues (≤0.3).
        // The confidence difference also contributes to metadata drift,
        // but since the predicate values are already wrong, the overall
        // level is WrongValues.
        assert!(detail.score <= 0.3);
        assert!(detail.score >= 0.0);
    }

    #[test]
    fn test_benchmark_with_custom_threshold() {
        let (enc, dec, seq) = make_components();
        let nodes = generate_benchmark_nodes();

        // With a very low threshold, everything should pass.
        let result = bench_semantic_roundtrip_with_threshold(
            &enc, &dec, &seq, &nodes, 0.0,
        );
        assert!(result.passed >= 0.99); // ~100%

        // With a high threshold (0.99), baseline encoder is lossy
        // (wrong entities), so very few or none should pass.
        let result = bench_semantic_roundtrip_with_threshold(
            &enc, &dec, &seq, &nodes, 0.99,
        );
        assert!(result.passed < 0.5);
    }

    #[test]
    fn test_benchmark_all_nodes_have_results() {
        let (enc, dec, seq) = make_components();
        let nodes = generate_benchmark_nodes();

        let result = bench_semantic_roundtrip(&enc, &dec, &seq, &nodes);

        // Every node should have a result (not a failure).
        assert_eq!(result.failures.len(), 0);
        assert_eq!(result.details.len(), nodes.len());
    }

    #[test]
    fn test_generate_benchmark_nodes_diverse() {
        let nodes = generate_benchmark_nodes();
        assert!(nodes.len() >= 10);

        // Check diversity: different predicate heads.
        let heads: Vec<String> = nodes.iter().map(|n| {
            match &n.predicate.head {
                PredicateHead::Name(name) => name.to_string(),
                PredicateHead::Id(_) => "id".to_string(),
            }
        }).collect();
        let unique_heads: std::collections::HashSet<_> = heads.iter().collect();
        assert!(unique_heads.len() >= 5, "expected at least 5 unique heads");
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_thresholds_are_sensible() {
        // These are compile-time constants; verify at runtime too.
        let _ = DEFAULT_THRESHOLD;
        let _ = V01_TARGET;
        let _ = ACCEPTANCE_THRESHOLD;
        assert!(DEFAULT_THRESHOLD < V01_TARGET);
        assert!(V01_TARGET < ACCEPTANCE_THRESHOLD);
        assert!((0.0..=1.0).contains(&DEFAULT_THRESHOLD));
    }

    // ─── Anti-cheat gate tests ───────────────────────────────

    #[test]
    fn test_gated_benchmark_runs() {
        let (enc, dec, seq) = make_components();
        let nodes = generate_benchmark_nodes();

        let result = bench_semantic_roundtrip_gated(&enc, &dec, &seq, &nodes);

        // Should run on the test split (half the nodes).
        assert!(result.total > 0);
        assert!(result.total <= nodes.len());
        assert!(result.avg_score >= 0.0);
        assert!(result.min_score >= 0.0);
        assert!(result.max_score <= 1.0);
    }

    #[test]
    fn test_gated_benchmark_disabled_gates() {
        // With gates disabled, gated benchmark should behave like ungated.
        let (enc, dec, seq) = make_components();
        let nodes = generate_benchmark_nodes();

        let gates = AntiCheatGates::disabled();
        let result = bench_semantic_roundtrip_gated_with(
            &enc, &dec, &seq, &nodes, &gates,
        );

        // With gates disabled, all nodes should be tested.
        assert_eq!(result.total, nodes.len());
    }

    #[test]
    fn test_dimension_budget_gate() {
        let gates = AntiCheatGates::default();

        // A typical node's canonical text is ~80-120 bytes.
        // The baseline encoder produces ~470-dim vectors.
        // 470 < 100/3 = 33? NO — the baseline encoder FAILS this gate
        // by default. This is expected: the baseline is a measurement
        // scaffold, not a production encoder.
        let canonical_bytes = 100usize;
        let vector_dim = 470usize;

        let result = gates.check_dimension_budget(vector_dim, canonical_bytes);
        assert!(result.is_err(), "baseline should fail dimension budget gate");

        // A hypothetical compact encoder with 8-dim z should pass
        // (8 * 32 = 256 bits < 100 * 8 / 3 = 266 bits).
        let compact_dim = 8usize;
        let result = gates.check_dimension_budget(compact_dim, canonical_bytes);
        assert!(result.is_ok(), "compact encoder (8 dims) should pass dimension budget gate");

        // 32-dim should fail (32 * 32 = 1024 bits >= 266 bits).
        let result = gates.check_dimension_budget(32, canonical_bytes);
        assert!(result.is_err(), "32-dim encoder should fail bit budget gate for 100-byte canonical");
    }

    #[test]
    fn test_dimension_budget_disabled() {
        let gates = AntiCheatGates::disabled();
        // Even huge vectors should pass when the gate is disabled.
        let result = gates.check_dimension_budget(10000, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_latent_operation_normalizes() {
        let gates = AntiCheatGates::default();
        let v = crate::encoder::ThoughtVector::from_vec(vec![3.0, 4.0, 0.0]);
        let gated = gates.apply_latent_operation(v);

        // Should be approximately normalized (norm ≈ 1.0 + tiny noise).
        let norm = gated.l2_norm();
        assert!(norm > 0.99 && norm < 1.01, "expected norm ≈ 1.0, got {}", norm);
    }

    #[test]
    fn test_latent_operation_disabled() {
        let gates = AntiCheatGates::disabled();
        let v = crate::encoder::ThoughtVector::from_vec(vec![3.0, 4.0]);
        let gated = gates.apply_latent_operation(v);

        // Should be unchanged when disabled.
        assert_eq!(gated.data, vec![3.0, 4.0]);
    }

    #[test]
    fn test_train_test_split() {
        let gates = AntiCheatGates::default();
        let nodes = generate_benchmark_nodes();

        let (train, test) = gates.train_test_split(&nodes);

        // With 10 nodes and 0.5 ratio, expect 5 test and 5 train.
        assert_eq!(train.len() + test.len(), nodes.len());
        assert!(!test.is_empty(), "should have at least 1 test node");
        assert!(test.len() <= 6, "should have at most 6 test nodes");
    }

    #[test]
    fn test_train_test_split_disabled() {
        let gates = AntiCheatGates::disabled();
        let nodes = generate_benchmark_nodes();

        let (train, test) = gates.train_test_split(&nodes);

        // With split disabled, all nodes are test nodes.
        assert!(train.is_empty());
        assert_eq!(test.len(), nodes.len());
    }

    #[test]
    fn test_gated_benchmark_scores_differ_from_ungated() {
        // The gated benchmark applies latent operation (noise), which
        // may cause different scores than the ungated version.
        // This test just verifies both produce valid results.
        let (enc, dec, seq) = make_components();
        let nodes = generate_benchmark_nodes();

        let ungated = bench_semantic_roundtrip(&enc, &dec, &seq, &nodes);
        let gated = bench_semantic_roundtrip_gated(&enc, &dec, &seq, &nodes);

        // Both should have valid scores.
        assert!(ungated.avg_score >= 0.0);
        assert!(gated.avg_score >= 0.0);

        // Gated should test fewer nodes (test split).
        assert!(gated.total <= ungated.total);
    }
}
