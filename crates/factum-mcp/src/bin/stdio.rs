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
use factum_rt::store::FactumStore;
use factum_mcp::handler::McpHandler;
use factum_mcp::protocol::JsonRpcRequest;

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

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                });
                let _ = writeln!(stdout, "{}", serde_json::to_string(&err).unwrap_or_default());
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
