//! Factum MCP Server — stdio transport.
//!
//! Reads JSON-RPC messages from stdin, writes responses to stdout.
//! Each message is a single line of JSON (newline-delimited JSON-RPC).
//!
//! Usage:
//!   cargo run -p factum-mcp --bin factum-mcp-server
//!
//! Test with echo + jq:
//!   echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"factum":{}}}}' | cargo run -p factum-mcp --bin factum-mcp-server

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use factum_core::morphemes::MorphemeRegistry;
use factum_mcp::handler::McpHandler;
use factum_mcp::protocol::JsonRpcRequest;
use factum_rt::store::FactumStore;

fn main() {
    let registry = Arc::new(MorphemeRegistry::with_seeds());
    let store = Arc::new(FactumStore::new(registry));
    let handler = McpHandler::new(store);

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": { "code": -32700, "message": "Parse error" }
                });
                let _ = writeln!(stdout, "{err}");
                let _ = stdout.flush();
                continue;
            }
        };

        // MCP notifications have no id and MUST NOT receive a response.
        // No notification handlers are needed for the currently advertised capabilities.
        if value["jsonrpc"] == "2.0" && value["method"].is_string() && value.get("id").is_none() {
            continue;
        }
        let req = match serde_json::from_value::<JsonRpcRequest>(value) {
            Ok(req)
                if req.jsonrpc == "2.0"
                    && (req.id.is_string() || req.id.is_i64() || req.id.is_u64()) =>
            {
                req
            }
            _ => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": { "code": -32600, "message": "Invalid Request" }
                });
                let _ = writeln!(stdout, "{err}");
                let _ = stdout.flush();
                continue;
            }
        };

        let resp = handler.handle(&req);
        let resp_json = serde_json::to_string(&resp).unwrap_or_default();
        let _ = writeln!(stdout, "{}", resp_json);
        let _ = stdout.flush();
    }
}
