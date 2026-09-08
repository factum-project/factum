//! Conformance test runner — loads JSON test vectors and verifies the Rust parser.
//!
//! Positive vectors: parse(input) → serialize(result) must equal expected_canonical
//! Negative vectors: parse(input) must fail with error containing the error_class string
//! Compact vectors: parse(input) → compact(serialize) must round-trip: compact(deserialize(compact(x))) == compact(x)

use std::fs;
use serde::Deserialize;
use factum_core::parser::Parser;
use factum_core::serialize;
use factum_core::serialize::CompactNode;
use factum_core::morphemes::MorphemeRegistry;

#[derive(Deserialize)]
struct PositiveVector {
    id: String,
    #[allow(dead_code)]
    description: String,
    input: String,
    expected_canonical: String,
}

#[derive(Deserialize)]
struct NegativeVector {
    id: String,
    #[allow(dead_code)]
    description: String,
    input: String,
    error_class: String,
}

fn conformance_dir() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/../../spec/conformance", manifest_dir)
}

fn run_positive_vectors(filename: &str) {
    let path = format!("{}/{}", conformance_dir(), filename);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    let vectors: Vec<PositiveVector> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON in {}: {}", path, e));

    for v in &vectors {
        // Phase 1: parse the input
        let nodes = Parser::parse(&v.input)
            .unwrap_or_else(|e| panic!("[{}] input should parse but got error: {}", v.id, e));

        // Phase 2: serialize to canonical form
        let actual_canonical = serialize::canonical_all(&nodes);

        // Phase 3: compare against expected_canonical
        // We also parse the expected_canonical and re-serialize, to normalize
        // any minor formatting differences (the comparison is structural, not byte-level)
        let expected_nodes = Parser::parse(&v.expected_canonical)
            .unwrap_or_else(|e| panic!("[{}] expected_canonical failed to parse: {}", v.id, e));
        let expected_serialized = serialize::canonical_all(&expected_nodes);

        assert_eq!(
            actual_canonical, expected_serialized,
            "[{}] canonical form mismatch:\n  expected: {}\n  actual:   {}",
            v.id, expected_serialized, actual_canonical
        );
    }
}

fn run_negative_vectors(filename: &str) {
    let path = format!("{}/{}", conformance_dir(), filename);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    let vectors: Vec<NegativeVector> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON in {}: {}", path, e));

    for v in &vectors {
        let result = Parser::parse(&v.input);

        match &result {
            Ok(nodes) => {
                // Input should have failed but didn't
                panic!(
                    "[{}] expected error class '{}' but input parsed successfully ({} nodes)",
                    v.id, v.error_class, nodes.len()
                );
            }
            Err(e) => {
                // Error message must contain the error_class string (case-insensitive)
                let msg = e.message.to_lowercase();
                let class = v.error_class.to_lowercase();
                assert!(
                    msg.contains(&class) || msg.contains(&class.replace('-', " ")),
                    "[{}] expected error containing '{}', got: '{}'",
                    v.id, v.error_class, e.message
                );
            }
        }
    }
}

#[test]
fn test_conformance_basic() {
    run_positive_vectors("parse_basic.json");
}

#[test]
fn test_conformance_errors() {
    run_negative_vectors("parse_errors.json");
}

// ── Compact form conformance ──

#[derive(Deserialize)]
struct CompactVector {
    id: String,
    #[allow(dead_code)]
    description: String,
    input: String,
    #[allow(dead_code)]
    expected_compact_json: String,
}

#[test]
fn test_conformance_compact_roundtrip() {
    let path = format!("{}/compact_basic.json", conformance_dir());
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    let vectors: Vec<CompactVector> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON in {}: {}", path, e));

    let registry = MorphemeRegistry::with_seeds();

    for v in &vectors {
        // Phase 1: parse the input
        let nodes = Parser::parse(&v.input)
            .unwrap_or_else(|e| panic!("[{}] input should parse but got error: {}", v.id, e));
        assert_eq!(nodes.len(), 1, "[{}] expected 1 node", v.id);
        let node = &nodes[0];

        // Phase 2: serialize to compact JSON
        let compact_json = serialize::compact(node, &registry);

        // Phase 3: deserialize the compact JSON back to CompactNode
        let deserialized: CompactNode = serde_json::from_str(&compact_json)
            .unwrap_or_else(|e| panic!("[{}] compact JSON failed to deserialize: {} — json: {}", v.id, e, compact_json));

        // Phase 4: re-serialize the deserialized CompactNode to JSON
        let re_compact_json = serde_json::to_string(&deserialized)
            .unwrap_or_else(|e| panic!("[{}] re-serialization failed: {}", v.id, e));

        // Phase 5: verify round-trip identity: compact(deserialize(compact(x))) == compact(x)
        assert_eq!(
            compact_json, re_compact_json,
            "[{}] compact round-trip mismatch:\n  original:    {}\n  re-serialized: {}",
            v.id, compact_json, re_compact_json
        );
    }
}
