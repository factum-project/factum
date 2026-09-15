//! Factum MCP Server — stdio transport.
//!
//! Reads JSON-RPC messages from stdin, writes responses to stdout.
//! Each message is a single line of JSON (newline-delimited JSON-RPC).
//!
//! ## Usage
//!
//! ### In-memory (default, data lost on exit)
//! ```bash
//! factum-mcp-server
//! ```
//!
//! ### Persistent (RocksDB, survives restarts)
//! ```bash
//! factum-mcp-server --db-path ~/.factum/store
//! ```
//!
//! ### Test with echo + jq
//! ```bash
//! echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"factum":{}}}}' | factum-mcp-server
//! ```

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use factum_core::morphemes::MorphemeRegistry;
use factum_mcp::handler::McpHandler;
use factum_mcp::protocol::JsonRpcRequest;
use factum_rt::store::FactumStore;

/// Print usage to stderr.
fn print_usage() {
    eprintln!("Usage: factum-mcp-server [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --db-path <PATH>    Use RocksDB persistence at the given directory.");
    eprintln!("                       The directory is created if it does not exist.");
    eprintln!("                       Data survives process restarts.");
    eprintln!("  --in-memory         Force in-memory mode (default). Data is lost on exit.");
    eprintln!("  --help, -h          Print this help message.");
    eprintln!();
    eprintln!("When no --db-path is given, the server runs in-memory (default).");
}

/// Parse command-line arguments and return the optional RocksDB path.
fn parse_args() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut db_path: Option<std::path::PathBuf> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--in-memory" => {
                db_path = None;
            }
            "--db-path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --db-path requires a value");
                    std::process::exit(1);
                }
                db_path = Some(std::path::PathBuf::from(&args[i]));
            }
            other => {
                eprintln!("error: unknown argument '{other}'");
                eprintln!();
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    db_path
}

fn main() {
    let db_path = parse_args();
    let registry = Arc::new(MorphemeRegistry::with_seeds());

    #[cfg(feature = "rocksdb")]
    let store = if let Some(path) = &db_path {
        eprintln!("factum-mcp-server: RocksDB mode at {}", path.display());
        match FactumStore::with_rocksdb(path, registry.clone()) {
            Ok(store) => store,
            Err(e) => {
                eprintln!("error: failed to open RocksDB at {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("factum-mcp-server: in-memory mode (data will be lost on exit)");
        FactumStore::new(registry)
    };

    #[cfg(not(feature = "rocksdb"))]
    let store = {
        if db_path.is_some() {
            eprintln!("error: --db-path requires building with --features rocksdb");
            eprintln!("hint: rebuild with: cargo build --release -p factum-mcp --features rocksdb");
            std::process::exit(1);
        }
        eprintln!("factum-mcp-server: in-memory mode (data will be lost on exit)");
        FactumStore::new(registry)
    };

    let handler = McpHandler::new(Arc::new(store));

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
