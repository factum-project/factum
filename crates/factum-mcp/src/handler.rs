//! MCP request handler — processes JSON-RPC messages.
//!
//! Routes incoming MCP requests to the appropriate Factum runtime operations.

use std::collections::HashMap;
use std::sync::Arc;
use serde_json;
use smol_str::SmolStr;
use factum_core::parser::Parser;
use factum_core::serialize;
use factum_core::types::*;
use factum_core::calibration;
use factum_rt::store::FactumStore;
use factum_rt::query::{Query, QueryOptions};
use factum_rt::arbitration::ConflictPolicy;
use factum_rt::permission::PermissionContext;
use crate::protocol::*;
use crate::tools::*;

/// Extract predicate head as string for display/comparison.
fn predicate_head_str(head: &PredicateHead) -> &str {
    match head {
        PredicateHead::Name(name) => name.as_str(),
        PredicateHead::Id(_) => "<morpheme-id>",
    }
}

/// Map a StoreError to the appropriate JSON-RPC error code.
///
/// - `AlreadyExists` and `NotFound` are client errors (invalid_params, -32602):
///   the caller provided a node ID that conflicts or doesn't exist.
/// - `PermissionDenied` is also a client error (invalid_params, -32602).
/// - `InvalidNode` is a client error (invalid_params, -32602).
/// - `Storage` is a server error (internal, -32603).
fn store_error_to_jsonrpc(e: &factum_rt::store::StoreError) -> JsonRpcError {
    match e {
        factum_rt::store::StoreError::NotFound(_)
        | factum_rt::store::StoreError::AlreadyExists(_)
        | factum_rt::store::StoreError::PermissionDenied
        | factum_rt::store::StoreError::InvalidNode(_) => {
            JsonRpcError::invalid_params(e.to_string())
        }
        factum_rt::store::StoreError::Storage(_) => {
            JsonRpcError::internal(e.to_string())
        }
    }
}

/// Generate a content-based node ID from canonical predicate text.
///
/// Format: "auto-" + first 12 hex chars of SHA-256(canonical text).
/// Same content → same ID (prevents accidental duplicates).
/// Different content → different ID (no collision in practice with 48-bit prefix).
fn generate_content_id(canonical_text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    canonical_text.hash(&mut hasher);
    let hash = hasher.finish();
    format!("auto-{:012x}", hash & 0xFFFFFFFFFFFF)
}

/// Parse the `agent_weights` JSON parameter into a `HashMap<String, f32>`.
///
/// Accepts a JSON object like `{"agent-a": 0.5, "agent-b": 0.3}`.
/// Returns an empty map if the parameter is None or not an object.
fn parse_agent_weights(value: &Option<serde_json::Value>) -> HashMap<String, f32> {
    match value {
        Some(serde_json::Value::Object(map)) => {
            map.iter()
                .filter_map(|(k, v)| {
                    if let serde_json::Value::Number(n) = v {
                        n.as_f64().map(|f| (k.clone(), f as f32))
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => HashMap::new(),
    }
}

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
            "ping" => JsonRpcResponse::success(req.id.clone(), serde_json::json!({})),
            "tools/list" => self.handle_list_tools(req),
            "resources/list" => self.handle_list_resources(req),
            "resources/templates/list" => JsonRpcResponse::success(req.id.clone(), serde_json::json!({
                "resourceTemplates": [{
                    "uriTemplate": "factum://nodes/{id}",
                    "name": "Factum node",
                    "description": "Read an active public knowledge node as canonical Factum text",
                    "mimeType": "text/plain"
                }]
            })),
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

        // Plain clients do not have a morpheme table, so return readable canonical text.
        *self.preferred_form.write() = if client_factum_aware {
            PreferredForm::Compact
        } else {
            PreferredForm::Canonical
        };

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
            // Canonical output needs no custom vocabulary negotiation.
            None
        };

        let result = InitializeResult {
            protocolVersion: MCP_PROTOCOL_VERSION.into(),
            capabilities: ServerCapabilities {
                tools: ToolCapability { listChanged: Some(true) },
                resources: ResourceCapability {
                    subscribe: None,
                    listChanged: None,
                },
            },
            serverInfo: ServerInfo {
                name: "factum-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
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
            "factum_lookup" => self.tool_lookup(req, &arguments),
            "factum_insert" => self.tool_insert(req, &arguments),
            "factum_insert_batch" => self.tool_insert_batch(req, &arguments),
            "factum_upsert" => self.tool_upsert(req, &arguments),
            "factum_assert" => self.tool_assert(req, &arguments),
            "factum_search" => self.tool_search(req, &arguments),
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
                "weighted" => {
                    let weights = parse_agent_weights(&params.agent_weights);
                    ConflictPolicy::WeightedVote { weights }
                }
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

                let tool_result = ToolResult::structured(json);
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
                    structuredContent: None,
                    isError: Some(false),
                };
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            Err(e) => JsonRpcResponse::error(req.id.clone(),
                store_error_to_jsonrpc(&e)),
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
                let tool_result = ToolResult::structured(json);
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            Err(e) => JsonRpcResponse::error(req.id.clone(),
                store_error_to_jsonrpc(&e)),
        }
    }

    /// Execute factum_lookup tool — look up all knowledge about an entity.
    fn tool_lookup(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumLookupParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        // Strip leading @ if user included it
        let entity_name = params.entity.strip_prefix('@').unwrap_or(&params.entity);
        let entity = EntityId::new(entity_name);

        // Use the by_entity index for fast lookup
        let mut nodes = self.store.lookup_by_entity(&entity);

        // Filter by min_confidence if specified
        if let Some(min_conf) = params.min_confidence {
            nodes.retain(|n| n.confidence.0 >= min_conf);
        }

        // Filter by permission (public) and active status
        let ctx = PermissionContext::public();
        nodes.retain(|n| n.status == NodeStatus::Active && self.store.check_permission(n, &ctx));

        // Serialize based on preferred_form
        let pf = *self.preferred_form.read();
        let json = match pf {
            PreferredForm::Canonical => {
                let node_strs: Vec<String> = nodes.iter()
                    .map(|n| serialize::canonical(n))
                    .collect();
                serde_json::json!({
                    "nodes": node_strs,
                    "form": "canonical",
                    "count": nodes.len(),
                })
            }
            PreferredForm::Compact => {
                let node_strs: Vec<_> = nodes.iter()
                    .map(|n| serialize::compact(n, self.store.registry()))
                    .collect();
                serde_json::json!({
                    "nodes": node_strs,
                    "count": nodes.len(),
                })
            }
        };

        let tool_result = ToolResult::structured(json);
        JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(tool_result).unwrap(),
        )
    }

    /// Execute factum_insert_batch tool — insert multiple nodes atomically.
    fn tool_insert_batch(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumInsertBatchParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        // Parse all nodes first — if any fails, reject the entire batch
        let mut all_nodes = Vec::with_capacity(params.nodes.len());
        for (i, node_str) in params.nodes.iter().enumerate() {
            match Parser::parse(node_str) {
                Ok(n) => all_nodes.extend(n),
                Err(e) => return JsonRpcResponse::error(req.id.clone(),
                    JsonRpcError::invalid_params(format!(
                        "Node {} parse error: {} (batch rejected)", i, e
                    ))),
            }
        }

        if all_nodes.is_empty() {
            return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params("no nodes parsed from batch"));
        }

        // Use insert_batch for atomic insertion
        match self.store.insert_batch(all_nodes) {
            Ok(()) => {
                let json = serde_json::json!({
                    "inserted": params.nodes.len(),
                    "status": "all nodes inserted successfully"
                });
                let tool_result = ToolResult::structured(json);
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            Err(e) => JsonRpcResponse::error(req.id.clone(),
                store_error_to_jsonrpc(&e)),
        }
    }

    /// Execute factum_upsert tool — update or insert a node.
    ///
    /// Finds active nodes matching entity + predicate, then:
    /// - 0 matches: plain insert (no retract needed)
    /// - 1 match: insert new node, then retract old node
    /// - 2+ matches: return Ambiguous (refuses to guess)
    ///
    /// **Non-atomic**: insert and retract are separate operations. If insert
    /// succeeds but retract fails, both nodes will exist (returned as
    /// `action: "partial"`). This is not data loss — the user has two versions
    /// and can manually retract the old one. True atomic insert+retract would
    /// require a combined WriteBatch API in FactumStore (future work).
    fn tool_upsert(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumUpsertParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        // Parse the new node first — fail early if invalid
        let nodes = match Parser::parse(&params.node) {
            Ok(n) => n,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!("Node parse error: {}", e))),
        };
        if nodes.is_empty() {
            return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params("no nodes parsed"));
        }
        let new_node = nodes.into_iter().next().unwrap();

        // Find existing active nodes matching entity + predicate
        let entity_name = params.entity.strip_prefix('@').unwrap_or(&params.entity);
        let entity = EntityId::new(entity_name);
        let ctx = PermissionContext::public();

        let matching: Vec<_> = self.store.lookup_by_entity(&entity)
            .into_iter()
            .filter(|n| {
                n.status == NodeStatus::Active
                    && self.store.check_permission(n, &ctx)
                    && predicate_head_str(&n.predicate.head) == params.predicate
            })
            .collect();

        match matching.len() {
            0 => {
                // No existing node — plain insert
                match self.store.insert(new_node) {
                    Ok(()) => {
                        let json = serde_json::json!({
                            "action": "inserted",
                            "retracted": [],
                            "status": "no existing node found, inserted as new"
                        });
                        let tool_result = ToolResult::structured(json);
                        JsonRpcResponse::success(
                            req.id.clone(),
                            serde_json::to_value(tool_result).unwrap(),
                        )
                    }
                    Err(e) => JsonRpcResponse::error(req.id.clone(),
                        store_error_to_jsonrpc(&e)),
                }
            }
            1 => {
                // Exactly one match — insert new, then retract old
                let old_node_id = matching[0].id.clone();

                // Insert new node first (if insert fails, old node is untouched)
                match self.store.insert(new_node) {
                    Ok(()) => {
                        // Now retract the old node
                        match self.store.retract(&old_node_id) {
                            Ok(retracted) => {
                                let json = serde_json::json!({
                                    "action": "updated",
                                    "retracted": retracted.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                                    "retracted_count": retracted.len(),
                                    "status": "old node retracted, new node inserted"
                                });
                                let tool_result = ToolResult::structured(json);
                                JsonRpcResponse::success(
                                    req.id.clone(),
                                    serde_json::to_value(tool_result).unwrap(),
                                )
                            }
                            Err(e) => {
                                // Insert succeeded but retract failed — both nodes exist
                                // This is not data loss, but the user has two versions
                                let json = serde_json::json!({
                                    "action": "partial",
                                    "warning": "new node inserted but old node retraction failed",
                                    "error": e.to_string(),
                                    "old_node_id": old_node_id.to_string(),
                                });
                                let tool_result = ToolResult::structured(json);
                                JsonRpcResponse::success(
                                    req.id.clone(),
                                    serde_json::to_value(tool_result).unwrap(),
                                )
                            }
                        }
                    }
                    Err(e) => JsonRpcResponse::error(req.id.clone(),
                        store_error_to_jsonrpc(&e)),
                }
            }
            _ => {
                // Multiple matches — Ambiguous, refuse to guess
                let node_ids: Vec<_> = matching.iter()
                    .map(|n| n.id.to_string())
                    .collect();
                let json = serde_json::json!({
                    "action": "ambiguous",
                    "matching_nodes": node_ids,
                    "count": matching.len(),
                    "status": "multiple matching nodes found — refusing to guess. Retract specific nodes manually or narrow the predicate."
                });
                let tool_result = ToolResult::structured(json);
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
        }
    }

    /// Execute factum_assert tool — assert a fact with minimal syntax.
    ///
    /// Parses a predicate S-expression, auto-generates a content-based node ID,
    /// assigns default provenance (Asserted), and inserts.
    /// - Node ID = "auto-" + first 12 hex chars of SHA-256(canonical predicate text)
    /// - Same content → same ID → second insert fails with AlreadyExists (prevents duplicates)
    /// - Different content → different ID (no collision in practice)
    fn tool_assert(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumAssertParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        // Parse the predicate S-expression
        let predicate = match Parser::parse_predicate(&params.predicate) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!("Predicate parse error: {}", e))),
        };

        // Generate content-based node ID
        let canon = serialize::canonical_predicate(&predicate);
        let node_id = generate_content_id(&canon);

        // Build node with defaults
        let mut node = Node::new(node_id.clone(), predicate);

        // Set provenance
        let by = params.by.unwrap_or_else(|| "system".to_string());
        node.provenance = Provenance::Asserted {
            by: Principal(SmolStr::new(by)),
        };

        // Set confidence: use explicit value if provided, else provenance-based default.
        // The default replaces the old Confidence::default() = 1.0, which was
        // scientifically unjustified. See docs/confidence-calibration-research.md.
        if let Some(conf) = params.confidence {
            node.confidence = Confidence(conf);
            // M2+ will enforce band clipping here (check_confidence_band).
            // For now, we apply the default and emit no warning — but the
            // band check function is available for future use.
        } else {
            node.confidence = calibration::default_confidence_for_provenance(&node.provenance);
        }

        // Insert
        match self.store.insert(node) {
            Ok(()) => {
                let json = serde_json::json!({
                    "action": "asserted",
                    "node_id": node_id,
                    "predicate": canon,
                    "status": "fact asserted with auto-generated ID"
                });
                let tool_result = ToolResult::structured(json);
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            Err(e) => JsonRpcResponse::error(req.id.clone(),
                store_error_to_jsonrpc(&e)),
        }
    }

    /// Execute factum_search tool — search nodes by keyword, list predicates, or get stats.
    fn tool_search(&self, req: &JsonRpcRequest, args: &serde_json::Value) -> JsonRpcResponse {
        let params: FactumSearchParams = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(e.to_string())),
        };

        let ctx = PermissionContext::public();
        let active_nodes: Vec<_> = self.store.all_active().into_iter()
            .filter(|n| self.store.check_permission(n, &ctx))
            .collect();

        match params.mode.as_str() {
            "keyword" => {
                let keyword = match &params.keyword {
                    Some(k) if !k.is_empty() => k,
                    _ => return JsonRpcResponse::error(req.id.clone(),
                        JsonRpcError::invalid_params("keyword is required when mode=\"keyword\"")),
                };
                let limit = params.limit.unwrap_or(50).min(200);
                let kw_lower = keyword.to_lowercase();

                // Serialize each node once, then filter+collect from the cached text
                let matches: Vec<String> = active_nodes.iter()
                    .map(|n| serialize::canonical(n))
                    .filter(|canon| canon.to_lowercase().contains(&kw_lower))
                    .take(limit)
                    .collect();

                let total = matches.len();
                let json = serde_json::json!({
                    "mode": "keyword",
                    "keyword": keyword,
                    "results": matches,
                    "count": total,
                    "truncated": total == limit,
                });
                let tool_result = ToolResult::structured(json);
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            "predicates" => {
                // List all distinct predicate heads with counts
                use std::collections::BTreeMap;
                let mut pred_counts: BTreeMap<String, usize> = BTreeMap::new();
                for n in &active_nodes {
                    *pred_counts.entry(predicate_head_str(&n.predicate.head).to_string()).or_insert(0) += 1;
                }
                let preds: Vec<_> = pred_counts.into_iter()
                    .map(|(head, count)| {
                        serde_json::json!({"predicate": head, "count": count})
                    })
                    .collect();
                let json = serde_json::json!({
                    "mode": "predicates",
                    "predicates": preds,
                    "distinct_count": preds.len(),
                    "total_active": active_nodes.len(),
                });
                let tool_result = ToolResult::structured(json);
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            "stats" => {
                let total = self.store.len();
                let active = active_nodes.len();
                let retracted = total - active;

                use std::collections::BTreeMap;
                let mut pred_counts: BTreeMap<String, usize> = BTreeMap::new();
                for n in &active_nodes {
                    *pred_counts.entry(predicate_head_str(&n.predicate.head).to_string()).or_insert(0) += 1;
                }

                let json = serde_json::json!({
                    "mode": "stats",
                    "total_nodes": total,
                    "active_nodes": active,
                    "retracted_nodes": retracted,
                    "distinct_predicates": pred_counts.len(),
                    "predicates": pred_counts.into_iter()
                        .map(|(head, count)| {
                            serde_json::json!({"predicate": head, "count": count})
                        })
                        .collect::<Vec<_>>(),
                });
                let tool_result = ToolResult::structured(json);
                JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(tool_result).unwrap(),
                )
            }
            _ => JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params(format!(
                    "unknown mode: {} (expected: keyword, predicates, or stats)", params.mode
                ))),
        }
    }

    /// List active nodes visible to the same public principal used by queries.
    fn handle_list_resources(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let ctx = PermissionContext::public();
        let mut nodes: Vec<_> = self.store.all_active().into_iter()
            .filter(|node| self.store.check_permission(node, &ctx))
            .collect();
        nodes.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        let resources: Vec<_> = nodes.iter().map(|node| serde_json::json!({
            "uri": node_resource_uri(node.id.as_str()),
            "name": node.id.as_str(),
            "mimeType": "text/plain",
        })).collect();
        JsonRpcResponse::success(req.id.clone(), serde_json::json!({"resources": resources}))
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

        match self.store.get_with_perm(&NodeId::new(id), &PermissionContext::public()) {
            Ok(node) if node.status == NodeStatus::Active => {
                JsonRpcResponse::success(req.id.clone(), serde_json::json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "text/plain",
                        "text": serialize::canonical(&node),
                    }]
                }))
            }
            _ => JsonRpcResponse::error(req.id.clone(),
                JsonRpcError::invalid_params("resource not found or not accessible")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(result["tools"].as_array().unwrap().len(), 8);
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
        let content = resp.result.unwrap()["structuredContent"].clone();
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
        let content = resp.result.unwrap()["structuredContent"].clone();
        // Compact form: no "form" field present
        assert!(content.get("form").is_none());
        assert!(content["count"].as_u64().unwrap_or(0) > 0);
    }

    // ─── Tool unit tests ───────────────────────────────

    fn call_tool(handler: &McpHandler, id: i64, name: &str, args: serde_json::Value) -> JsonRpcResponse {
        handler.handle(&JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(id),
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": name,
                "arguments": args
            })),
        })
    }

    #[test]
    fn test_tool_insert_success() {
        let handler = make_handler();
        let resp = call_tool(&handler, 10, "factum_insert",
            json!({"node": "(node n010 :pred (located-in @ACME-CORP @SHENZHEN))"}));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["isError"], false);
    }

    #[test]
    fn test_tool_insert_duplicate_returns_invalid_params() {
        let handler = make_handler();
        // n001 already exists in make_handler
        let resp = call_tool(&handler, 11, "factum_insert",
            json!({"node": "(node n001 :pred (instance-of @X @Y))"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602); // invalid_params, not internal
    }

    #[test]
    fn test_tool_insert_parse_error() {
        let handler = make_handler();
        let resp = call_tool(&handler, 12, "factum_insert",
            json!({"node": "(node n012 :pred (unclosed"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_tool_retract_success() {
        let handler = make_handler();
        let resp = call_tool(&handler, 20, "factum_retract",
            json!({"node_id": "n001"}));
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result["structuredContent"]["count"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn test_tool_retract_not_found_returns_invalid_params() {
        let handler = make_handler();
        let resp = call_tool(&handler, 21, "factum_retract",
            json!({"node_id": "nonexistent"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602); // invalid_params
    }

    #[test]
    fn test_tool_lookup_by_entity() {
        let handler = make_handler();
        let resp = call_tool(&handler, 30, "factum_lookup",
            json!({"entity": "ACME-CORP"}));
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result["structuredContent"]["count"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn test_tool_lookup_no_match() {
        let handler = make_handler();
        let resp = call_tool(&handler, 31, "factum_lookup",
            json!({"entity": "NONEXISTENT"}));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["structuredContent"]["count"], 0);
    }

    #[test]
    fn test_tool_insert_batch_success() {
        let handler = make_handler();
        let resp = call_tool(&handler, 40, "factum_insert_batch",
            json!({"nodes": [
                "(node n010 :pred (located-in @X @Y))",
                "(node n011 :pred (founded-on @X #date(2000-01-01)))"
            ]}));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["structuredContent"]["inserted"], 2);
    }

    #[test]
    fn test_tool_insert_batch_parse_error_rejects_all() {
        let handler = make_handler();
        let resp = call_tool(&handler, 41, "factum_insert_batch",
            json!({"nodes": [
                "(node n010 :pred (located-in @X @Y))",
                "(node n011 :pred (unclosed"
            ]}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_tool_upsert_insert_when_no_match() {
        let handler = make_handler();
        let resp = call_tool(&handler, 50, "factum_upsert",
            json!({
                "node": "(node n020 :pred (version @SOME-ENTITY \"1.0\"))",
                "entity": "SOME-ENTITY",
                "predicate": "version"
            }));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["structuredContent"]["action"], "inserted");
    }

    #[test]
    fn test_tool_upsert_update_when_one_match() {
        let handler = make_handler();
        // First insert a version node
        call_tool(&handler, 51, "factum_insert",
            json!({"node": "(node n021 :pred (version @ENT \"1.0\"))"}));
        // Now upsert to update it
        let resp = call_tool(&handler, 52, "factum_upsert",
            json!({
                "node": "(node n022 :pred (version @ENT \"2.0\"))",
                "entity": "ENT",
                "predicate": "version"
            }));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["structuredContent"]["action"], "updated");
    }

    #[test]
    fn test_tool_upsert_ambiguous_when_multiple_match() {
        let handler = make_handler();
        // Insert two version nodes for same entity
        call_tool(&handler, 53, "factum_insert",
            json!({"node": "(node n023 :pred (version @ENT2 \"1.0\"))"}));
        call_tool(&handler, 54, "factum_insert",
            json!({"node": "(node n024 :pred (version @ENT2 \"2.0\"))"}));
        // Upsert should be ambiguous
        let resp = call_tool(&handler, 55, "factum_upsert",
            json!({
                "node": "(node n025 :pred (version @ENT2 \"3.0\"))",
                "entity": "ENT2",
                "predicate": "version"
            }));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["structuredContent"]["action"], "ambiguous");
    }

    #[test]
    fn test_tool_assert_success() {
        let handler = make_handler();
        let resp = call_tool(&handler, 60, "factum_assert",
            json!({"predicate": "(version @TEST-ASSERT \"1.0\")"}));
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        let node_id = result["structuredContent"]["node_id"].as_str().unwrap();
        assert!(node_id.starts_with("auto-"));
        assert_eq!(result["structuredContent"]["action"], "asserted");
    }

    #[test]
    fn test_tool_assert_duplicate_returns_invalid_params() {
        let handler = make_handler();
        // First assert
        call_tool(&handler, 61, "factum_assert",
            json!({"predicate": "(version @TEST-DUP \"1.0\")"}));
        // Second assert with same content → same ID → AlreadyExists
        let resp = call_tool(&handler, 62, "factum_assert",
            json!({"predicate": "(version @TEST-DUP \"1.0\")"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602); // invalid_params
    }

    #[test]
    fn test_tool_assert_parse_error() {
        let handler = make_handler();
        let resp = call_tool(&handler, 63, "factum_assert",
            json!({"predicate": "not a predicate"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_tool_assert_with_custom_by_and_confidence() {
        let handler = make_handler();
        let resp = call_tool(&handler, 64, "factum_assert",
            json!({
                "predicate": "(status @TEST-BY active)",
                "by": "agent-1",
                "confidence": 0.8
            }));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["structuredContent"]["action"], "asserted");
    }

    #[test]
    fn test_tool_assert_default_confidence_is_provenance_based() {
        // factum_assert without :conf should use provenance-based default
        // (0.60 for Asserted), NOT the old default of 1.0.
        let handler = make_handler();
        let resp = call_tool(&handler, 65, "factum_assert",
            json!({"predicate": "(status @TEST-DEFAULT-CONF checked)"}));
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        let node_id = result["structuredContent"]["node_id"].as_str().unwrap();

        // Look up the inserted node and verify confidence
        let nodes: Vec<_> = handler.store.all_active();
        let node = nodes.iter().find(|n| n.id.as_str() == node_id).unwrap();
        assert_eq!(
            node.confidence,
            Confidence(0.60),
            "Asserted node without explicit :conf should default to 0.60, not 1.0"
        );
    }

    #[test]
    fn test_tool_search_keyword() {
        let handler = make_handler();
        let resp = call_tool(&handler, 70, "factum_search",
            json!({"mode": "keyword", "keyword": "ACME"}));
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result["structuredContent"]["count"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn test_tool_search_keyword_no_match() {
        let handler = make_handler();
        let resp = call_tool(&handler, 71, "factum_search",
            json!({"mode": "keyword", "keyword": "NONEXISTENTXYZ"}));
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["structuredContent"]["count"], 0);
    }

    #[test]
    fn test_tool_search_predicates() {
        let handler = make_handler();
        let resp = call_tool(&handler, 72, "factum_search",
            json!({"mode": "predicates"}));
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result["structuredContent"]["distinct_count"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn test_tool_search_stats() {
        let handler = make_handler();
        let resp = call_tool(&handler, 73, "factum_search",
            json!({"mode": "stats"}));
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result["structuredContent"]["total_nodes"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn test_tool_search_invalid_mode() {
        let handler = make_handler();
        let resp = call_tool(&handler, 74, "factum_search",
            json!({"mode": "invalid"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_tool_search_keyword_missing_keyword() {
        let handler = make_handler();
        let resp = call_tool(&handler, 75, "factum_search",
            json!({"mode": "keyword"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_store_error_to_jsonrpc_not_found() {
        let e = factum_rt::store::StoreError::NotFound("test".to_string());
        let err = store_error_to_jsonrpc(&e);
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn test_store_error_to_jsonrpc_already_exists() {
        let e = factum_rt::store::StoreError::AlreadyExists("test".to_string());
        let err = store_error_to_jsonrpc(&e);
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn test_store_error_to_jsonrpc_storage() {
        let e = factum_rt::store::StoreError::Storage("test".to_string());
        let err = store_error_to_jsonrpc(&e);
        assert_eq!(err.code, -32603);
    }

    #[test]
    fn test_generate_content_id_deterministic() {
        let id1 = generate_content_id("(version @X \"1.0\")");
        let id2 = generate_content_id("(version @X \"1.0\")");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("auto-"));
    }

    #[test]
    fn test_generate_content_id_different_content() {
        let id1 = generate_content_id("(version @X \"1.0\")");
        let id2 = generate_content_id("(version @X \"2.0\")");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_list_changed_declared() {
        let handler = make_handler();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "initialize".into(),
            params: Some(serde_json::json!({"capabilities": {}})),
        };
        let resp = handler.handle(&req);
        let caps = resp.result.unwrap()["capabilities"].clone();
        assert_eq!(caps["tools"]["listChanged"], true);
    }
}
