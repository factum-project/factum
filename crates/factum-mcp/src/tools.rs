//! MCP tool definitions for Factum.
//!
//! Eight tools are exposed:
//! - `factum_query`: Query the knowledge graph by predicate pattern
//! - `factum_lookup`: Look up all knowledge about a specific entity
//! - `factum_insert`: Insert a single node (full syntax)
//! - `factum_insert_batch`: Insert multiple nodes atomically
//! - `factum_upsert`: Update or insert a node (retract old + insert new)
//! - `factum_assert`: Assert a fact with minimal syntax (auto node ID + provenance)
//! - `factum_search`: Search nodes by keyword, list predicates, or get stats
//! - `factum_retract`: Retract a node (cascade)
//!
//! Field names use camelCase to match the MCP wire format exactly.

#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

/// Tool definition (MCP format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub inputSchema: serde_json::Value,
}

/// Parameters for factum_query tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactumQueryParams {
    /// Factum-F S-expression query
    pub query: String,
    /// Conflict policy: "latest", "authority", "unanimous", "weighted"
    #[serde(default)]
    pub policy: Option<String>,
    /// "As of" historical query (ISO 8601 datetime)
    #[serde(default)]
    pub as_of: Option<String>,
    /// Minimum confidence threshold
    #[serde(default)]
    pub min_confidence: Option<f32>,
    /// Agent weights for "weighted" policy: { "agent-name": weight, ... }
    /// Weights are 0.0–1.0. Agents not listed default to 0.5.
    #[serde(default)]
    pub agent_weights: Option<serde_json::Value>,
}

/// Parameters for factum_insert tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactumInsertParams {
    /// Factum-F S-expression node definition
    pub node: String,
}

/// Parameters for factum_retract tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactumRetractParams {
    /// Node ID to retract
    pub node_id: String,
    /// Maximum number of nodes to retract in the cascade (including the
    /// root node). Default: 100. If the cascade exceeds this limit,
    /// it is truncated and the response includes `truncated: true`.
    #[serde(default)]
    pub max_cascade_nodes: Option<usize>,
}

/// Parameters for factum_lookup tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactumLookupParams {
    /// Entity reference (without @ prefix), e.g. "ACME-CORP"
    pub entity: String,
    /// Minimum confidence threshold
    #[serde(default)]
    pub min_confidence: Option<f32>,
}

/// Parameters for factum_insert_batch tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactumInsertBatchParams {
    /// Array of Factum-F node definitions
    pub nodes: Vec<String>,
}

/// Parameters for factum_upsert tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactumUpsertParams {
    /// Factum-F S-expression node definition for the new node
    pub node: String,
    /// Entity reference to find existing nodes to retract (without @ prefix)
    pub entity: String,
    /// Predicate head to match (e.g. "version", "description")
    pub predicate: String,
}

/// Parameters for factum_search tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactumSearchParams {
    /// Search mode: "keyword" (substring match), "predicates" (list all predicate heads), "stats" (counts)
    pub mode: String,
    /// Keyword for substring search (required when mode="keyword")
    #[serde(default)]
    pub keyword: Option<String>,
    /// Maximum results (default 50, max 200)
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Parameters for factum_assert tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactumAssertParams {
    /// Predicate S-expression, e.g. "(version @FACTUM-PROJECT \"0.1.3\")"
    pub predicate: String,
    /// Who is asserting this fact (default: "system")
    #[serde(default)]
    pub by: Option<String>,
    /// Confidence level 0.0–1.0. If omitted, a provenance-based default
    /// is used (Asserted=0.60, Extracted=0.80, etc.).
    #[serde(default, alias = "conf")]
    pub confidence: Option<f32>,
    /// Optional human-readable note for context (e.g. "v6 plan")
    #[serde(default)]
    pub note: Option<String>,
}

/// Get all tool definitions.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "factum_query".into(),
            description: "Query the Factum knowledge graph using S-expression syntax. Returns matching nodes with their provenance and confidence.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Factum-F S-expression query, e.g. (shareholder-major @ACME-CORP ?p)"
                    },
                    "policy": {
                        "type": "string",
                        "enum": ["latest", "authority", "unanimous", "weighted"],
                        "default": "latest",
                        "description": "Conflict resolution policy. 'weighted' uses agent_weights for majority vote (multi-agent)."
                    },
                    "as_of": {
                        "type": "string",
                        "format": "date-time",
                        "description": "Historical query: return knowledge valid at this time"
                    },
                    "min_confidence": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "default": 0,
                        "description": "Minimum confidence threshold"
                    },
                    "agent_weights": {
                        "type": "object",
                        "description": "Agent weights for 'weighted' policy. Keys are agent/principal names, values are 0.0–1.0. Agents not listed default to 0.5.",
                        "additionalProperties": { "type": "number" }
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "factum_insert".into(),
            description: "Insert a new knowledge node into the Factum graph. The node must be in valid Factum-F S-expression format. Use \"auto\" as the node ID to auto-generate a content-based ID (same behavior as factum_assert).".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node": {
                        "type": "string",
                        "description": "Factum-F node definition, e.g. (node n001 :pred (instance-of @X organization)). Use (node auto :pred ...) to auto-generate the node ID."
                    }
                },
                "required": ["node"]
            }),
        },
        ToolDefinition {
            name: "factum_retract".into(),
            description: "Retract a node from the Factum graph. Retraction is a soft delete — the node is marked as retracted but not removed, preserving audit history. Derived nodes depending on the retracted node are cascade-retracted. The cascade is limited to a configurable number of nodes to prevent explosion in large knowledge bases.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": {
                        "type": "string",
                        "description": "ID of the node to retract"
                    },
                    "max_cascade_nodes": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 100,
                        "description": "Maximum number of nodes to retract in the cascade (including root). If exceeded, the cascade is truncated and 'truncated: true' is returned."
                    }
                },
                "required": ["node_id"]
            }),
        },
        ToolDefinition {
            name: "factum_lookup".into(),
            description: "Look up all knowledge about a specific entity. Returns all active nodes where the entity appears in the predicate arguments. Uses exact entity name matching (e.g. \"ACME-CORP\" matches only @ACME-CORP, not @ACME-SUBSIDIARY). For partial/substring matching, use factum_search with mode=\"keyword\" instead.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "Entity name (without @ prefix), e.g. \"ACME-CORP\""
                    },
                    "min_confidence": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "default": 0,
                        "description": "Minimum confidence threshold"
                    }
                },
                "required": ["entity"]
            }),
        },
        ToolDefinition {
            name: "factum_insert_batch".into(),
            description: "Insert multiple knowledge nodes atomically. If any node fails parsing or verification, the entire batch is rejected (no partial insert). More efficient than calling factum_insert repeatedly.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "nodes": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "maxItems": 100,
                        "description": "Array of Factum-F node definitions"
                    }
                },
                "required": ["nodes"]
            }),
        },
        ToolDefinition {
            name: "factum_upsert".into(),
            description: "Update or insert a knowledge node. Finds active nodes matching the given entity + predicate, retracts them, then inserts the new node. If multiple matching nodes exist, returns Ambiguous (refuses to guess). If no match, performs a plain insert.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node": {
                        "type": "string",
                        "description": "Factum-F node definition for the new node, e.g. (node n010 :pred (version @FACTUM-PROJECT \"0.1.2\"))"
                    },
                    "entity": {
                        "type": "string",
                        "description": "Entity name to find existing nodes (without @ prefix), e.g. \"FACTUM-PROJECT\""
                    },
                    "predicate": {
                        "type": "string",
                        "description": "Predicate head to match, e.g. \"version\""
                    }
                },
                "required": ["node", "entity", "predicate"]
            }),
        },
        ToolDefinition {
            name: "factum_search".into(),
            description: "Search the knowledge graph by keyword, list all predicates, or get statistics. Keyword mode finds nodes whose canonical text contains the keyword (case-insensitive). Predicates mode lists all distinct predicate heads with counts. Stats mode returns total node count and status breakdown.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["keyword", "predicates", "stats"],
                        "description": "Search mode: keyword (substring search), predicates (list all predicate heads), stats (counts summary)"
                    },
                    "keyword": {
                        "type": "string",
                        "description": "Keyword for substring search (required when mode=\"keyword\")"
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "default": 50,
                        "description": "Maximum results (keyword mode only)"
                    }
                },
                "required": ["mode"]
            }),
        },
        ToolDefinition {
            name: "factum_assert".into(),
            description: "Assert a fact with minimal syntax. Provide only the predicate S-expression — node ID is auto-generated from content hash, provenance defaults to Asserted. For full control (validity, deps, custom permissions), use factum_insert.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "predicate": {
                        "type": "string",
                        "description": "Predicate S-expression, e.g. (version @FACTUM-PROJECT \"0.1.3\")"
                    },
                    "by": {
                        "type": "string",
                        "default": "system",
                        "description": "Who is asserting this fact"
                    },
                    "confidence": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Confidence level. If omitted, a provenance-based default is used (Asserted=0.60, Extracted=0.80, etc.). See docs/confidence-calibration-research.md."
                    },
                    "note": {
                        "type": "string",
                        "description": "Optional human-readable note for context (e.g. \"v6 plan\", \"extracted from page 3\"). Does not affect content hash or equality."
                    }
                },
                "required": ["predicate"]
            }),
        },
    ]
}

/// Resource URI pattern: factum://nodes/{id}
pub fn node_resource_uri(id: &str) -> String {
    format!("factum://nodes/{}", id)
}

/// Extract node ID from resource URI.
pub fn parse_node_resource_uri(uri: &str) -> Option<&str> {
    uri.strip_prefix("factum://nodes/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definitions() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 8);
        assert!(tools.iter().any(|t| t.name == "factum_query"));
        assert!(tools.iter().any(|t| t.name == "factum_insert"));
        assert!(tools.iter().any(|t| t.name == "factum_retract"));
        assert!(tools.iter().any(|t| t.name == "factum_lookup"));
        assert!(tools.iter().any(|t| t.name == "factum_insert_batch"));
        assert!(tools.iter().any(|t| t.name == "factum_upsert"));
        assert!(tools.iter().any(|t| t.name == "factum_search"));
        assert!(tools.iter().any(|t| t.name == "factum_assert"));
    }

    #[test]
    fn test_query_schema() {
        let tools = tool_definitions();
        let query_tool = tools.iter().find(|t| t.name == "factum_query").unwrap();
        let schema = &query_tool.inputSchema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"]["type"] == "string");
    }

    #[test]
    fn test_resource_uri() {
        assert_eq!(node_resource_uri("n001"), "factum://nodes/n001");
        assert_eq!(parse_node_resource_uri("factum://nodes/n001"), Some("n001"));
        assert_eq!(parse_node_resource_uri("other://n001"), None);
    }
}
