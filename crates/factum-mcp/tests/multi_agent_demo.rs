//! End-to-end integration test for the multi-agent demo scenario.
//!
//! This test verifies that the scenarios described in
//! `docs/demo/multi-agent-demo.md` actually work. It exercises the
//! full handler path: JSON-RPC → tool dispatch → store → query → arbitration.
//!
//! If this test fails, the demo documentation must be updated.

use factum_core::morphemes::MorphemeRegistry;
use factum_mcp::handler::McpHandler;
use factum_mcp::protocol::*;
use factum_rt::store::FactumStore;
use serde_json::{json, Value};
use std::sync::Arc;

fn make_handler() -> McpHandler {
    let registry = Arc::new(MorphemeRegistry::with_seeds());
    let store = FactumStore::new(registry);
    McpHandler::new(Arc::new(store))
}

fn call_tool(handler: &McpHandler, id: i64, name: &str, args: Value) -> JsonRpcResponse {
    handler.handle(&JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: serde_json::Value::Number(id.into()),
        method: "tools/call".into(),
        params: Some(json!({
            "name": name,
            "arguments": args,
        })),
    })
}

/// Extract the structured content from a successful response.
fn structured(resp: JsonRpcResponse) -> Value {
    resp.result
        .expect("expected success response")
        .get("structuredContent")
        .cloned()
        .unwrap_or(json!({}))
}

/// Step 1: Two agents assert different values for the same attribute.
/// This must NOT return an error — both assertions should succeed.
#[test]
fn test_demo_step1_conflicting_assertions_succeed() {
    let handler = make_handler();

    // Agent A asserts declining
    let resp_a = call_tool(&handler, 1, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP declining)",
        "by": "analyst-a",
        "confidence": 0.80
    }));
    assert!(resp_a.error.is_none(), "Agent A assert should succeed");
    assert_eq!(structured(resp_a)["action"], "asserted");

    // Agent B asserts growing (different value, different content hash)
    let resp_b = call_tool(&handler, 2, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP growing)",
        "by": "analyst-b",
        "confidence": 0.60
    }));
    assert!(resp_b.error.is_none(), "Agent B assert should succeed");
    assert_eq!(structured(resp_b)["action"], "asserted");
}

/// Step 1b: Corroboration — same fact, different principal → success (not error)
#[test]
fn test_demo_step1b_corroboration_returns_success() {
    let handler = make_handler();

    // Agent A asserts a fact
    call_tool(&handler, 10, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP declining)",
        "by": "analyst-a"
    }));

    // Agent C asserts the same fact
    let resp_c = call_tool(&handler, 11, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP declining)",
        "by": "analyst-c"
    }));

    assert!(resp_c.error.is_none(), "corroboration must not be an error");
    let content = structured(resp_c);
    assert_eq!(content["action"], "corroborated");
    assert_eq!(content["corroborated_by"], "analyst-a");
}

/// Step 1c: Same principal re-asserting → idempotent (not error)
#[test]
fn test_demo_step1c_same_principal_idempotent() {
    let handler = make_handler();

    call_tool(&handler, 20, "factum_assert", json!({
        "predicate": "(status @SERVER-1 healthy)",
        "by": "agent-x"
    }));

    let resp = call_tool(&handler, 21, "factum_assert", json!({
        "predicate": "(status @SERVER-1 healthy)",
        "by": "agent-x"
    }));

    assert!(resp.error.is_none(), "same principal re-assert should be no-op");
    assert_eq!(structured(resp)["action"], "duplicate");
}

/// Step 3: WeightedVote resolves value conflict.
/// Two agents assert different values; weighted vote picks the majority.
#[test]
fn test_demo_step3_weighted_vote_resolves_conflict() {
    let handler = make_handler();

    // Agent A (weight 0.5): declining
    call_tool(&handler, 30, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP declining)",
        "by": "analyst-a"
    }));

    // Agent B (weight 0.2): growing
    call_tool(&handler, 31, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP growing)",
        "by": "analyst-b"
    }));

    // Query with WeightedVote
    let resp = call_tool(&handler, 32, "factum_query", json!({
        "query": "(revenue-trend @ACME-CORP ?trend)",
        "policy": "weighted",
        "agent_weights": {
            "analyst-a": 0.5,
            "analyst-b": 0.2
        }
    }));

    assert!(resp.error.is_none(), "weighted vote query should succeed");
    let content = structured(resp);
    // Should resolve to declining (weight 0.5 > 0.2)
    assert_eq!(
        content["ambiguous"], false,
        "WeightedVote must resolve when one side has majority"
    );
    assert_eq!(
        content["count"].as_u64(),
        Some(1),
        "WeightedVote must return exactly 1 result on resolution"
    );
}

/// Step 4: Equal weights → Ambiguous (refuse to answer)
#[test]
fn test_demo_step4_equal_weights_ambiguous() {
    let handler = make_handler();

    // Agent A: declining
    call_tool(&handler, 40, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP declining)",
        "by": "analyst-a"
    }));

    // Agent B: growing (same weight)
    call_tool(&handler, 41, "factum_assert", json!({
        "predicate": "(revenue-trend @ACME-CORP growing)",
        "by": "analyst-b"
    }));

    // Equal weights → neither exceeds 50%
    let resp = call_tool(&handler, 42, "factum_query", json!({
        "query": "(revenue-trend @ACME-CORP ?trend)",
        "policy": "weighted",
        "agent_weights": {
            "analyst-a": 0.5,
            "analyst-b": 0.5
        }
    }));

    assert!(resp.error.is_none());
    let content = structured(resp);
    assert_eq!(
        content["ambiguous"], true,
        "equal weights with disagreement must be ambiguous"
    );
}

/// Step 5: Cascade retraction with node limit
#[test]
fn test_demo_step5_cascade_retraction() {
    let handler = make_handler();

    // Insert a base node
    let resp = call_tool(&handler, 50, "factum_assert", json!({
        "predicate": "(revenue @ACME-CORP 1000000)",
        "by": "analyst-a"
    }));
    let node_id = structured(resp)["node_id"].as_str().unwrap().to_string();

    // Insert a derived node that depends on it
    call_tool(&handler, 51, "factum_insert", json!({
        "node": format!(
            "(node n-derived :pred (growth-rate @ACME-CORP 0.15) :src (derived {} \"rule-1\") :deps [{}] :conf 0.70)",
            node_id, node_id
        )
    }));

    // Retract the source
    let resp = call_tool(&handler, 52, "factum_retract", json!({
        "node_id": node_id,
        "max_cascade_nodes": 50
    }));

    assert!(resp.error.is_none());
    let content = structured(resp);
    assert!(content["count"].as_u64().unwrap() >= 2, "cascade should retract at least 2 nodes");
    assert_eq!(content["truncated"], false, "small cascade should not be truncated");
}

/// Parameter name validation: "conf" alias works for "confidence"
#[test]
fn test_demo_conf_alias_accepted() {
    let handler = make_handler();
    let resp = call_tool(&handler, 60, "factum_assert", json!({
        "predicate": "(status @ALIAS-TEST checked)",
        "conf": 0.75
    }));
    assert!(resp.error.is_none(), "conf alias should be accepted");
}

/// Unknown fields are rejected (not silently ignored)
#[test]
fn test_demo_unknown_fields_rejected() {
    let handler = make_handler();
    let resp = call_tool(&handler, 70, "factum_assert", json!({
        "predicate": "(status @UNKNOWN-X y)",
        "bogus_field": 123
    }));
    assert!(resp.error.is_some(), "unknown fields must be rejected");
}
