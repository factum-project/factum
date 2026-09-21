//! HTTP transport integration tests.
//!
//! Tests the factum-mcp-http binary's HTTP endpoint by sending raw
//! HTTP requests over TCP. These tests require the `http` feature
//! and are gated behind `cfg(feature = "http")`.

#![cfg(feature = "http")]

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use std::sync::atomic::{AtomicU16, Ordering};

static NEXT_PORT: AtomicU16 = AtomicU16::new(18080);

/// Pick an unused port for the test server.
fn next_port() -> u16 {
    NEXT_PORT.fetch_add(1, Ordering::SeqCst)
}

/// Start the HTTP server on a random port, return the child + URL.
struct ServerHandle {
    child: Child,
    addr: String,
}

fn start_server() -> ServerHandle {
    let port = next_port();
    let addr = format!("127.0.0.1:{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_factum-mcp-http"))
        .arg("--addr")
        .arg(&addr)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to start factum-mcp-http");

    // Wait for server to be ready
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("server did not start within 10s");
        }
        if TcpStream::connect_timeout(
            &addr.parse().unwrap(),
            Duration::from_millis(500),
        ).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    ServerHandle { child, addr }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// Send a POST request and return the response body as a JSON Value.
fn post_json(addr: &str, body: &Value) -> Value {
    let body_str = serde_json::to_string(body).unwrap();
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(10))).unwrap();

    let request = format!(
        "POST /mcp HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body_str}",
        body_str.len()
    );

    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    // Parse HTTP response: skip headers, find the JSON body
    let body_start = response.find("\r\n\r\n").expect("no body separator");
    let json_body = &response[body_start + 4..];

    // Handle chunked encoding if present
    if response.contains("transfer-encoding: chunked") || response.contains("Transfer-Encoding: chunked") {
        // Simple chunked decoder: skip chunk size lines
        let trimmed = json_body.trim();
        if let Some(pos) = trimmed.find('\n') {
            let _chunk_size = &trimmed[..pos];
            let rest = &trimmed[pos + 1..];
            return serde_json::from_str(rest.trim()).expect("failed to parse chunked JSON");
        }
    }

    serde_json::from_str(json_body.trim()).expect("failed to parse JSON response")
}

/// Send a GET request and return the response body as a string.
fn get_text(addr: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();

    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Connection: close\r\n\
         \r\n"
    );

    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    let body_start = response.find("\r\n\r\n").expect("no body separator");
    response[body_start + 4..].trim().to_string()
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

fn init_request() -> Value {
    rpc(1, "initialize", json!({"capabilities":{"factum":{}}}))
}

#[test]
fn test_http_health() {
    let server = start_server();
    let body = get_text(&server.addr, "/health");
    assert_eq!(body, "ok");
}

#[test]
fn test_http_initialize() {
    let server = start_server();
    let resp = post_json(&server.addr, &init_request());
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["protocolVersion"].is_string());
    assert_eq!(resp["result"]["serverInfo"]["name"], "factum-mcp");
    // Factum-aware client should get morpheme table
    assert!(resp["result"]["factum_morphemes"].is_array());
}

#[test]
fn test_http_tools_list() {
    let server = start_server();
    // Must initialize first
    let _ = post_json(&server.addr, &init_request());
    // Then list tools
    let resp = post_json(&server.addr, &rpc(2, "tools/list", json!({})));
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 9);
    // Verify factum_review is present
    let names: Vec<&str> = tools.iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"factum_review"));
}

#[test]
fn test_http_insert_and_query() {
    let server = start_server();
    let _ = post_json(&server.addr, &init_request());

    // Insert a node
    let insert_resp = post_json(&server.addr, &rpc(2, "tools/call", json!({
        "name": "factum_insert",
        "arguments": {
            "node": "(node auto :pred (instance-of @OpenAI @Organization))"
        }
    })));
    assert_eq!(insert_resp["id"], 2);
    assert!(insert_resp["result"]["structuredContent"]["status"].as_str().unwrap().contains("inserted"), "insert failed: {insert_resp}");

    // Query it
    let query_resp = post_json(&server.addr, &rpc(3, "tools/call", json!({
        "name": "factum_query",
        "arguments": {
            "query": "(instance-of ?x ?y)"
        }
    })));
    assert_eq!(query_resp["id"], 3);
    let sc = &query_resp["result"]["structuredContent"];
    assert!(sc.is_object(), "no structuredContent in query response: {query_resp}");
    let count = sc["count"].as_u64().unwrap_or(0);
    assert!(count > 0, "query returned no results: {query_resp}");
}

#[test]
fn test_http_concurrent_requests() {
    let server = start_server();
    let _ = post_json(&server.addr, &init_request());

    // Spawn 10 threads that each insert a different node
    let addr = server.addr.clone();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let addr = addr.clone();
            std::thread::spawn(move || {
                let node = format!("(node auto :pred (instance-of @Agent{i} @Organization))");
                let resp = post_json(&addr, &rpc(
                    i + 100,
                    "tools/call",
                    json!({
                        "name": "factum_insert",
                        "arguments": {"node": node}
                    }),
                ));
                assert!(resp["result"]["structuredContent"]["status"].as_str().unwrap_or("").contains("inserted"),
                    "concurrent insert {i} failed: {resp}");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify all 10 are present
    let query_resp = post_json(&server.addr, &rpc(200, "tools/call", json!({
        "name": "factum_query",
        "arguments": {"query": "(instance-of ?x ?y)"}
    })));
    let count = query_resp["result"]["structuredContent"]["count"]
        .as_u64().unwrap_or(0);
    assert!(count >= 10, "expected >= 10 results, got {count}");
}

#[test]
fn test_http_review_queue() {
    let server = start_server();
    let _ = post_json(&server.addr, &init_request());

    // List pending review events (should be empty initially)
    let resp = post_json(&server.addr, &rpc(2, "tools/call", json!({
        "name": "factum_review",
        "arguments": {"mode": "list_pending"}
    })));
    assert_eq!(resp["id"], 2);
    let events = resp["result"]["structuredContent"]["pending"]
        .as_array().unwrap();
    assert!(events.is_empty(), "expected no pending events, got {events:?}");
}

#[test]
fn test_http_404_on_unknown_path() {
    let server = start_server();
    let mut stream = TcpStream::connect(&server.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = stream.write_all(b"GET /nonexistent HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    // axum returns 404 for unknown routes
    assert!(resp.contains("404") || resp.contains("Not Found"),
        "expected 404, got: {resp}");
}

#[test]
fn test_http_error_on_invalid_json() {
    let server = start_server();
    let mut stream = TcpStream::connect(&server.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();

    let bad_body = "not valid json";
    let request = format!(
        "POST /mcp HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {bad_body}",
        server.addr,
        bad_body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    // axum returns 400 Bad Request for invalid JSON
    assert!(response.contains("400") || response.contains("Bad Request"),
        "expected 400 for invalid JSON, got: {response}");
}
