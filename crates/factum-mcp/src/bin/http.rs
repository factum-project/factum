//! Factum MCP Server — Streamable HTTP transport.
//!
//! Serves MCP JSON-RPC over HTTP, enabling multiple agent clients to
//! share a single Factum store instance.
//!
//! ## Usage
//!
//! ### In-memory (default, data lost on exit)
//! ```bash
//! factum-mcp-http --addr 0.0.0.0:8080
//! ```
//!
//! ### Persistent (RocksDB, survives restarts)
//! ```bash
//! factum-mcp-http --addr 0.0.0.0:8080 --db-path ~/.factum/store
//! ```
//!
//! ### With CORS (for browser-based Inspector)
//! ```bash
//! factum-mcp-http --addr 0.0.0.0:8080 --cors-origins http://localhost:3000
//! ```
//!
//! ### Test with curl
//! ```bash
//! curl -X POST http://localhost:8080/mcp \
//!   -H 'Content-Type: application/json' \
//!   -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"factum":{}}}}'
//! ```

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;

use factum_core::morphemes::MorphemeRegistry;
use factum_mcp::handler::McpHandler;
use factum_mcp::protocol::JsonRpcRequest;
use factum_rt::store::FactumStore;

/// Application state shared across all HTTP handlers.
struct AppState {
    handler: McpHandler,
}

/// Parse command-line arguments.
struct CliArgs {
    addr: String,
    db_path: Option<std::path::PathBuf>,
    cors_origins: Vec<String>,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut addr = String::from("0.0.0.0:8080");
    let mut db_path: Option<std::path::PathBuf> = None;
    let mut cors_origins: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--addr" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --addr requires a value");
                    std::process::exit(1);
                }
                addr = args[i].clone();
            }
            "--db-path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --db-path requires a value");
                    std::process::exit(1);
                }
                db_path = Some(std::path::PathBuf::from(&args[i]));
            }
            "--in-memory" => {
                db_path = None;
            }
            "--cors-origins" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --cors-origins requires at least one value");
                    std::process::exit(1);
                }
                // Comma-separated list
                cors_origins = args[i].split(',').map(|s| s.trim().to_string()).collect();
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

    CliArgs { addr, db_path, cors_origins }
}

fn print_usage() {
    eprintln!("Usage: factum-mcp-http [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --addr <ADDR>           Listen address and port (default: 0.0.0.0:8080)");
    eprintln!("  --db-path <PATH>        Use RocksDB persistence at the given directory.");
    eprintln!("                          The directory is created if it does not exist.");
    eprintln!("                          Data survives process restarts.");
    eprintln!("  --in-memory             Force in-memory mode (default). Data is lost on exit.");
    eprintln!("  --cors-origins <URLS>   Comma-separated allowed CORS origins (e.g. http://localhost:3000)");
    eprintln!("  --help, -h              Print this help message.");
    eprintln!();
    eprintln!("When no --db-path is given, the server runs in-memory (default).");
}

/// Build the FactumStore from CLI args.
fn build_store(db_path: Option<std::path::PathBuf>) -> FactumStore {
    let registry = Arc::new(MorphemeRegistry::with_seeds());

    #[cfg(feature = "rocksdb")]
    {
        if let Some(path) = &db_path {
            eprintln!("factum-mcp-http: RocksDB mode at {}", path.display());
            match FactumStore::with_rocksdb(path, registry) {
                Ok(store) => return store,
                Err(e) => {
                    eprintln!("error: failed to open RocksDB at {}: {}", path.display(), e);
                    std::process::exit(1);
                }
            }
        }
    }

    #[cfg(not(feature = "rocksdb"))]
    if db_path.is_some() {
        eprintln!("error: --db-path requires building with --features rocksdb");
        eprintln!("hint: rebuild with: cargo build --release -p factum-mcp --features http,rocksdb");
        std::process::exit(1);
    }

    eprintln!("factum-mcp-http: in-memory mode (data will be lost on exit)");
    FactumStore::new(registry)
}

/// POST /mcp — JSON-RPC request handler.
///
/// Accepts a JSON-RPC 2.0 request body, processes it through McpHandler,
/// and returns the JSON-RPC response.
async fn handle_mcp(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let resp = state.handler.handle(&req);
    (StatusCode::OK, Json(resp))
}

/// GET /health — simple health check endpoint.
async fn handle_health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

fn build_router(state: Arc<AppState>, cors_origins: &[String]) -> Router {
    let mut cors = CorsLayer::new();
    if !cors_origins.is_empty() {
        // Parse origins
        let origins: Vec<_> = cors_origins
            .iter()
            .filter_map(|o| o.parse::<HeaderValue>().ok())
            .collect();
        if !origins.is_empty() {
            cors = cors.allow_origin(origins);
            cors = cors.allow_headers([
                header::CONTENT_TYPE,
                header::HeaderName::from_static("mcp-session-id"),
            ]);
            cors = cors.allow_methods([
                axum::http::Method::POST,
                axum::http::Method::GET,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ]);
        }
    }

    Router::new()
        .route("/mcp", post(handle_mcp))
        .route("/health", axum::routing::get(handle_health))
        .layer(RequestBodyLimitLayer::new(1024 * 1024)) // 1MB max body
        .layer(cors)
        .with_state(state)
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let store = build_store(args.db_path);
    let handler = McpHandler::new(Arc::new(store));
    let state = Arc::new(AppState { handler });

    let app = build_router(state, &args.cors_origins);

    let listener = match tokio::net::TcpListener::bind(&args.addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind to {}: {}", args.addr, e);
            std::process::exit(1);
        }
    };

    eprintln!("factum-mcp-http: listening on http://{}", args.addr);
    eprintln!("  POST /mcp     — JSON-RPC endpoint");
    eprintln!("  GET  /health  — health check");
    if !args.cors_origins.is_empty() {
        eprintln!("  CORS origins: {}", args.cors_origins.join(", "));
    }

    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| {
            eprintln!("error: server failed: {e}");
            std::process::exit(1);
        });
}
