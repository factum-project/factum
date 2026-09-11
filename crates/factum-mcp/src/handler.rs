//! MCP request handler — processes JSON-RPC messages.
//!
//! Routes incoming MCP requests to the appropriate Factum runtime operations.

use std::sync::Arc;
use serde_json;
use factum_core::parser::Parser;
use factum_core::serialize;
use factum_core::types::*;
use factum_rt::store::FactumStore;
use factum_rt::query::{Query, QueryOptions};
use factum_rt::arbitration::ConflictPolicy;
use crate::protocol::*;
use crate::tools::*;

/// The form in which query results are serialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PreferredForm {
    /// Compact JSON with numeric tags (default, for service-to-service).
    #[default]
    Compact,
    /// Canonical S-expression text (for LLM context, saves 62% tokens).
    Canonical,
}

/// MCP handler — bridges MCP requests to the Factum runtime.
pub struct McpHandler {
    store: Arc<FactumStore>,
    /// The preferred serialization form, negotiated during initialize.
    preferred_form: parking_lot::RwLock<PreferredForm>,
}

impl McpHandler {
    /// Create a new handler with the given store.
    pub fn new(store: Arc<FactumStore>) -> Self {
        Self {
            store,
            preferred_form: parking_lot::RwLock::new(PreferredForm::default()),
        }
    }

    /// Process a JSON-RPC request and return a JSON-RPC response.
    pub fn handle(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        match req.method.as_str() {
            "initialize" => self.handle_initialize(req),
            "tools/list" => self.handle_list_tools(req),
            "tools/call" => self.handle_call_tool(req),
            "resources/read" => self.handle_read_resource(req),
            _ => JsonRpcResponse::error(req.id.clone(), JsonRpcError::method_not_found()),
        }
    }

    /// Handle the initialize handshake.
    ///
    /// The `factum_morphemes` field is a Factum-specific extension to MCP's
    /// InitializeResult. If the client declares `capabilities.factum` in
    /// its initialize request, we send the morpheme table. Otherwise, we
    /// omit it and the client will use string-name form for all morpheme
    /// references. See spec/compact-form.md §6.
    fn handle_initialize(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        // Check if the client declared the `factum` capability
        let factum_caps = req.params
            .as_ref()
            .and_then(|p| p.get("capabilities"))
            .and_then(|c| c.get("factum"));

        let client_factum_aware = factum_caps.is_some();

        // Negotiate preferred_form (spec/compact-form.md §8)
        if let Some(fc) = factum_caps {
            if let Some(pf) = fc.get("preferred_form").and_then(|v| v.as_str()) {
                match pf {
                    "canonical" => *self.preferred_form.write() = PreferredForm::Canonical,
                    "compact" => *self.preferred_form.write() = PreferredForm::Compact,
                    _ => {} // unknown value, keep default
                }
            }
        }

        // Send morpheme table only to Factum-aware clients
        let morphemes = if client_factum_aware {
            let table: Vec<MorphemeEntry> = self.store.registry().all().iter()
                .map(|m| MorphemeEntry {
                    id: m.id.0,
                    name: m.name.to_string(),
                    kind: format!("{:?}", m.kind),
                })
                .collect();
            Some(table)
        } else {
            // Vanilla MCP client: omit morpheme table.
            // Compact form will use string names (graceful degradation).
            None
        };

        let result = InitializeResult {
            protocolVersion: MCP_PROTOCOL_VERSION.into(),
            capabilities: ServerCapabilities {
                tools: ToolCapability { listChanged: Some(true) },
                resources: ResourceCapability {
                    subscribe: Some(true),
                    listChanged: Some(true),
                },
            },
            serverInfo: ServerInfo {
                name: "factum-mcp".into(),
                version: "0.1.0".into(),
            },
            factum_morphemes: morphemes,
        };

        JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(result).unwrap(),
        )
    }

    /// Handle tools/list — return available tools.
    fn handle_list_tools(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let tools = tool_definitions();
        let result = serde_json::json!({
            "tools": tools
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle tools/call — execute a tool.
    fn handle_call_tool(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &req.params {
            Some(p) => p,
            None => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params("missing params")),
        };

        let tool_name = params.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let arguments = params.get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        match tool_name {
            "factum_query" => self.tool_query(req, &arguments),
            "factum_insert" => self.tool_insert(req, &arguments),
            "factum_retract" => self.tool_retract(req, &arguments),
            _ => JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!("unknown tool: {}", tool_name))),
        }
    }

    /// Execute factum_query tool.
    fn tool_query(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumQueryParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        // Parse the query S-expression
        let pattern = match Parser::parse_predicate(&params.query) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!("Query parse error: {}", e))),
        };

        // Build query
        let q = Query::new(pattern);

        // Build options
        let mut opts = QueryOptions::default();
        if let Some(policy_str) = &params.policy {
            opts.policy = match policy_str.as_str() {
                "latest" => ConflictPolicy::LatestWins,
                "authority" => ConflictPolicy::HighestAuthority,
                "unanimous" => ConflictPolicy::Unanimous,
                _ => ConflictPolicy::LatestWins,
            };
        }
        if let Some(as_of) = &params.as_of {
            if let Ok(dt) = as_of.parse::<chrono::DateTime<chrono::Utc>>() {
                opts.now = dt;
            }
        }
        if let Some(conf) = params.min_confidence {
            opts.min_conf = Confidence(conf);
        }

        // Execute query
        match self.store.query(&q, &opts) {
            Ok(results) => {
                // Serialize results based on negotiated preferred_form
                let pf = *self.preferred_form.read();
                let json = match pf {
                    PreferredForm::Canonical => {
                        // Return canonical S-expression text for LLM clients
                        let nodes: Vec<String> = results.results.iter()
                            .map(|r| serialize::canonical(&r.node))
                            .collect();
                        serde_json::json!({
                            "nodes": nodes,
                            "form": "canonical",
                            "count": results.results.len(),
                            "ambiguous": results.ambiguous,
                        })
                    }
                    PreferredForm::Compact => {
                        // Return compact JSON for service-to-service transport
                        let nodes: Vec<_> = results.results.iter()
                            .map(|r| serialize::compact(&r.node, self.store.registry()))
                            .collect();
                        serde_json::json!({
                            "nodes": nodes,
                            "count": results.results.len(),
                            "ambiguous": results.ambiguous,
                        })
                    }
                };

                let tool_result = ToolResult {
                    content: vec![ContentBlock::json(json)],
                    isError: Some(false),
                };
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            Err(e) => JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::internal(e.to_string())),
        }
    }

    /// Execute factum_insert tool.
    fn tool_insert(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumInsertParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        // Parse the node S-expression
        let nodes = match Parser::parse(&params.node) {
            Ok(n) => n,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!("Node parse error: {}", e))),
        };

        if nodes.is_empty() {
            return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params("no nodes parsed"));
        }

        // Insert
        match self.store.insert(nodes.into_iter().next().unwrap()) {
            Ok(()) => {
                let tool_result = ToolResult {
                    content: vec![ContentBlock::text("Node inserted successfully")],
                    isError: Some(false),
                };
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            Err(e) => JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::internal(e.to_string())),
        }
    }

    /// Execute factum_retract tool.
    fn tool_retract(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumRetractParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        match self.store.retract(&NodeId::new(params.node_id)) {
            Ok(retracted) => {
                let json = serde_json::json!({
                    "retracted": retracted.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                    "count": retracted.len(),
                });
                let tool_result = ToolResult {
                    content: vec![ContentBlock::json(json)],
                    isError: Some(false),
                };
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            Err(e) => JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::internal(e.to_string())),
        }
    }

    /// Handle resources/read — read a node resource.
    fn handle_read_resource(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &req.params {
            Some(p) => p,
            None => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params("missing params")),
        };

        let uri = params.get("uri")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let id = match parse_node_resource_uri(uri) {
            Some(id) => id,
            None => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!("invalid resource URI: {}", uri))),
        };

        match self.store.get(&NodeId::new(id)) {
            Some(node) => {
                let canonical = serialize::canonical(&node);
                let tool_result = ToolResult {
                    content: vec![ContentBlock::text(canonical)],
                    isError: Some(false),
                };
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            None => JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!("node not found: {}", id))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handler() -> McpHandler {
        let store = Arc::new(FactumStore::with_seeds());
        // Insert some test data
        store.insert(Node::new("n001",
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("ACME-CORP"), Term::ent("organization")])).with_permissions(PermissionTag::PUBLIC)).unwrap();
        McpHandler::new(store)
    }

    #[test]
    fn test_initialize_factum_aware() {
        let handler = make_handler();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "capabilities": {"factum": {}}
            })),
        };

        let resp = handler.handle(&req);
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "factum-mcp");
        assert!(result["factum_morphemes"].is_array());
    }

    #[test]
    fn test_initialize_non_factum_aware() {
        // Vanilla MCP client (no factum capability) should NOT receive morpheme table
        let handler = make_handler();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "capabilities": {"tools": {}}
            })),
        };

        let resp = handler.handle(&req);
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "factum-mcp");
        // factum_morphemes should be absent (graceful degradation)
        assert!(result.get("factum_morphemes").is_none() || result["factum_morphemes"].is_null());
    }

    #[test]
    fn test_list_tools() {
        let handler = make_handler();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(2),
            method: "tools/list".into(),
            params: None,
        };

        let resp = handler.handle(&req);
        let result = resp.result.unwrap();
        assert!(result["tools"].is_array());
        assert_eq!(result["tools"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_query_tool() {
        let handler = make_handler();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(3),
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": "factum_query",
                "arguments": {
                    "query": "(instance-of @ACME-CORP ?type)"
                }
            })),
        };

        let resp = handler.handle(&req);
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_read_resource() {
        let handler = make_handler();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(4),
            method: "resources/read".into(),
            params: Some(serde_json::json!({
                "uri": "factum://nodes/n001"
            })),
        };

        let resp = handler.handle(&req);
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_unknown_method() {
        let handler = make_handler();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(5),
            method: "unknown/method".into(),
            params: None,
        };

        let resp = handler.handle(&req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn test_preferred_form_canonical() {
        let handler = make_handler();
        // Initialize with preferred_form: canonical
        let init_req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "capabilities": {"factum": {"preferred_form": "canonical"}}
            })),
        };
        handler.handle(&init_req);

        // Query — should return canonical form
        let query_req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(2),
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": "factum_query",
                "arguments": {"query": "(instance-of @ACME-CORP ?type)"}
            })),
        };
        let resp = handler.handle(&query_req);
        assert!(resp.result.is_some());
        let content = resp.result.unwrap()["content"][0]["json"].clone();
        assert_eq!(content["form"], "canonical");
        // Canonical nodes should be strings (S-expressions), not objects
        assert!(content["nodes"][0].is_string());
    }

    #[test]
    fn test_preferred_form_default_compact() {
        let handler = make_handler();
        // Initialize WITHOUT preferred_form — should default to compact
        let init_req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "capabilities": {"factum": {}}
            })),
        };
        handler.handle(&init_req);

        // Query — should return compact form (no "form" field)
        let query_req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(2),
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": "factum_query",
                "arguments": {"query": "(instance-of @ACME-CORP ?type)"}
            })),
        };
        let resp = handler.handle(&query_req);
        assert!(resp.result.is_some());
        let content = resp.result.unwrap()["content"][0]["json"].clone();
        // Compact form: no "form" field present
        assert!(content.get("form").is_none());
        assert!(content["count"].as_u64().unwrap_or(0) > 0);
    }
}
