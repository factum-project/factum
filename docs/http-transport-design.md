# HTTP Transport Design Document

**Status**: Design (not yet implemented)  
**ROADMAP**: M2 — Promotion-Ready  
**Target**: 2026 Q4

## Goal

Add Streamable HTTP transport to `factum-mcp`, enabling a single Factum
server instance to serve multiple MCP clients over the network. This is
the key enabler for real multi-agent scenarios beyond stdio's one-process
limitation.

## Current state

- `McpHandler` is transport-agnostic: it takes `JsonRpcRequest` → `JsonRpcResponse`
- stdio transport (`src/bin/stdio.rs`) reads newline-delimited JSON from stdin
- Each MCP client starts its own Factum process → separate stores (or RocksDB lock contention)
- No async runtime in the codebase (all synchronous)

## Design

### Framework choice: `axum`

| Option | Pros | Cons |
|--------|------|------|
| `axum` | Built on `hyper` + `tokio`, ergonomic routing, tower middleware, SSE support | Adds `tokio` dependency |
| `hyper` directly | Minimal dependencies | More boilerplate, manual routing |
| `actix-web` | Mature, fast | Different ecosystem, heavier |

**Recommendation**: `axum`. It's the de facto standard for Rust HTTP
servers, has first-class SSE support (needed for MCP streaming), and
integrates cleanly with `tokio`. The `tokio` dependency is unavoidable
for any async HTTP server.

### Architecture

```
HTTP Client (Agent A) ─┐
HTTP Client (Agent B) ─┼──→ axum server ──→ McpHandler ──→ FactumStore (Arc<RwLock>)
HTTP Client (Agent C) ─┘         │
                                ├── POST /mcp   (JSON-RPC request/response)
                                ├── GET  /mcp   (SSE stream for subscriptions)
                                └── DELETE /mcp (session teardown)
```

### API surface

**`POST /mcp`** — JSON-RPC over HTTP

Request body: standard JSON-RPC 2.0 message  
Response body: JSON-RPC 2.0 response

```http
POST /mcp HTTP/1.1
Content-Type: application/json

{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"factum_query","arguments":{"query":"(status @X ?y)"}}}
```

**`GET /mcp`** — SSE stream (future, for subscriptions)

Returns `text/event-stream` with subscription events. Requires MCP
session ID from `initialize`.

**Session management**:

MCP Streamable HTTP uses a session header (`Mcp-Session-Id`). The
server generates a UUID on `initialize` and tracks sessions in an
in-memory map. Each session binds to a `PermissionContext` (so
different agents can have different permission levels).

### Concurrency model

**Phase 1 (M2 initial)**: `Arc<FactumStore>` with existing `RwLock`.

`FactumStore` already uses `parking_lot::RwLock` internally:
- Multiple concurrent readers (queries) — no contention
- Single writer (inserts/retracts) — brief lock

This is sufficient for 10-50 concurrent agents with mixed read/write
patterns. `axum` handles each request on a separate tokio task, but
the `RwLock` serializes writes safely.

**Phase 2 (M2 later)**: MVCC optimistic concurrency.

For higher write contention, add MVCC with version numbers:
- Each node has a `version` field
- Write attempts check `current_version == expected_version`
- If mismatch → `StoreError::VersionConflict` → client retries
- Eliminates write lock contention entirely

### Binary layout

New binary: `factum-mcp-http`

```
crates/factum-mcp/
  src/
    bin/
      stdio.rs       (existing)
      http.rs        (new — axum server)
    handler.rs       (existing, shared)
    protocol.rs      (existing, shared)
    tools.rs         (existing, shared)
```

The `axum` dependency is feature-gated:

```toml
[features]
default = []
rocksdb = ["factum-rt/rocksdb"]
http = ["dep:axum", "dep:tokio", "dep:tower"]
```

### CLI

```bash
# HTTP server (in-memory)
factum-mcp-http --addr 0.0.0.0:8080

# HTTP server (RocksDB persistence)
factum-mcp-http --addr 0.0.0.0:8080 --db-path ~/.factum/store

# HTTP server with TLS
factum-mcp-http --addr 0.0.0.0:8443 --tls-cert cert.pem --tls-key key.pem
```

### Security considerations

1. **No authentication by default** (M2 initial). Factum is designed as
   a local/trusted network service. Authentication is the deployment's
   responsibility (reverse proxy, mTLS, etc.).

2. **CORS**: Disabled by default. Enable via `--cors-origins` flag for
   browser-based clients (Inspector prototype).

3. **Rate limiting**: `tower::timeout` + `tower::limit` middleware for
   request timeout and concurrency limits. Default: 30s timeout, 100
   concurrent requests.

4. **Request size limit**: 1MB max body size (configurable).

### Testing strategy

1. **Unit tests**: `McpHandler` already has 42 tests — all reusable
2. **Integration tests**: HTTP endpoint tests using `reqwest` or
   `axum::test` helpers
3. **Concurrent write test**: 10 parallel agents writing different
   nodes, verify all succeed
4. **Concurrent read test**: 100 parallel queries, verify response time
   <100ms p99
5. **Session isolation test**: Two sessions with different permissions,
   verify isolation

### Dependency additions

```toml
# In crates/factum-mcp/Cargo.toml
[dependencies]
# ... existing deps ...
axum = { version = "0.7", optional = true }
tokio = { version = "1", features = ["full"], optional = true }
tower = { version = "0.5", optional = true }
tower-http = { version = "0.6", optional = true, features = ["cors", "limit"] }
```

Estimated binary size increase: ~2MB (axum + tokio + hyper).

### Roadmap alignment

This design fulfills the M2 ROADMAP items:
- `📋 Streamable HTTP transport (requires web framework: axum or hyper)`
- Enables `📋 Real Claude Code integration test` (HTTP endpoint for testing)
- Prerequisite for `📋 Inspector prototype` (needs HTTP endpoint)
- Prerequisite for subscription MCP exposure (SSE transport)

### Non-goals

- WebSocket transport (SSE is sufficient for MCP streaming)
- Built-in authentication/RBAC (deployment concern)
- Load balancing (single server, use reverse proxy if needed)
- Horizontal scaling (single RocksDB instance, sharding is M3+ research)
