//! MCP tool definitions for Factum.
//!
//! Three tools are exposed:
//! - `factum_query`: Query the knowledge graph
//! - `factum_insert`: Insert a new node
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
    /// Conflict policy: "latest", "authority", "unanimous"
    #[serde(default)]
    pub policy: Option<String>,
    /// "As of" historical query (ISO 8601 datetime)
    #[serde(default)]
    pub as_of: Option<String>,
    /// Minimum confidence threshold
    #[serde(default)]
    pub min_confidence: Option<f32>,
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
                        "enum": ["latest", "authority", "unanimous"],
                        "default": "latest",
                        "description": "Conflict resolution policy when multiple sources provide conflicting information"
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
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "factum_insert".into(),
            description: "Insert a new knowledge node into the Factum graph. The node must be in valid Factum-F S-expression format.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node": {
                        "type": "string",
                        "description": "Factum-F node definition, e.g. (node n001 :pred (instance-of @X organization))"
                    }
                },
                "required": ["node"]
            }),
        },
        ToolDefinition {
            name: "factum_retract".into(),
            description: "Retract a node from the Factum graph. Retraction is a soft delete — the node is marked as retracted but not removed, preserving audit history. Derived nodes depending on the retracted node are cascade-retracted.".into(),
            inputSchema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": {
                        "type": "string",
                        "description": "ID of the node to retract"
                    }
                },
                "required": ["node_id"]
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
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "factum_query"));
        assert!(tools.iter().any(|t| t.name == "factum_insert"));
        assert!(tools.iter().any(|t| t.name == "factum_retract"));
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
