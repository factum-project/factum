# Getting Started with Factum MCP

This guide walks you through running the Factum MCP server and using it with
LLM clients. The server uses stdio transport (JSON-RPC over stdin/stdout).

## Prerequisites

- Rust 1.75+ (install via [rustup](https://rustup.rs))
- Git

## Step 1: Build the server

```bash
git clone https://github.com/factum-project/factum.git
cd factum
cargo build --release -p factum-mcp --bin factum-mcp-server
```

The binary will be at `target/release/factum-mcp-server`.

For development, use `cargo run` directly:

```bash
cargo run -p factum-mcp --bin factum-mcp-server
```

## Step 2: Verify it works

Pipe a JSON-RPC initialize request and check the response:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"factum":{}}}}' \
  | cargo run -p factum-mcp --bin factum-mcp-server 2>/dev/null
```

You should see a response with `protocolVersion`, `serverInfo`, and a
`factum_morphemes` array listing all 24 seed morphemes.

## Step 3: Insert knowledge and query

The server reads newline-delimited JSON from stdin. Each line is one
JSON-RPC request. Here's a complete insert + query flow:

```bash
cat <<'EOF' | cargo run -p factum-mcp --bin factum-mcp-server 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"factum":{}}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"factum_insert","arguments":{"node":"(node n001 :pred (instance-of @ACME-CORP organization) :conf 0.99 :auth 0.95 :perm public :src (asserted wikidata))"}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"factum_query","arguments":{"query":"(instance-of ?x organization)"}}}
EOF
```

Expected output (abbreviated):
```
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18",...}}
{"jsonrpc":"2.0","id":2,"result":{"content":[{"text":"Node inserted successfully","type":"text"}]}}
{"jsonrpc":"2.0","id":3,"result":{"content":[{"json":{"count":1,"nodes":["..."]}}]}}
```

## Step 4: List available tools

```bash
cat <<'EOF' | cargo run -p factum-mcp --bin factum-mcp-server 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
EOF
```

This returns three tools:
- `factum_query` — query the knowledge graph
- `factum_insert` — insert a new node
- `factum_retract` — retract a node (cascade)

## Step 5: Configure in Claude Code

Add the Factum MCP server to your Claude Code configuration:

```json
{
  "mcpServers": {
    "factum": {
      "command": "/path/to/factum/target/release/factum-mcp-server",
      "args": []
    }
  }
}
```

After adding, restart Claude Code. You should see `factum_query`,
`factum_insert`, and `factum_retract` as available tools.

## Step 6: Configure in Cursor

Add to your Cursor MCP settings (Settings > MCP Servers):

```json
{
  "mcpServers": {
    "factum": {
      "command": "/path/to/factum/target/release/factum-mcp-server",
      "args": []
    }
  }
}
```

## Example queries

### Insert a knowledge graph

```
factum_insert: (node n001 :pred (instance-of @ACME-CORP organization) :conf 0.99 :auth 0.95 :perm public :src (asserted wikidata))

factum_insert: (node n002 :pred (located-in @ACME-CORP @SHENZHEN) :conf 0.95 :auth 0.9 :perm public :src (asserted wikidata))

factum_insert: (node n003 :pred (founded-on @ACME-CORP #date(2001-03-15)) :conf 0.99 :auth 1.0 :perm public :src (verbatim doc001 [0 50]))

factum_insert: (node n004 :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73) :conf 0.85 :auth 0.8 :perm confidential :src (extracted doc002 [100 200] (model gpt-4 2024-06)))
```

### Query with variable binding

```
factum_query: (shareholder-major @ACME-CORP ?holder ?stake)
```

This returns all major shareholders of ACME-CORP with their stake amounts.

### Historical query ("as of" a specific time)

```
factum_query with as_of: "2020-01-01T00:00:00Z"
query: (located-in @ACME-CORP ?loc)
```

### Retract with cascade

```
factum_retract: n001
```

If n006 was derived from n001 (via `:deps [n001]`), it will be
cascade-retracted automatically.

## How it works

```
LLM Client (Claude/Cursor)
    ↕ JSON-RPC 2.0 (stdio)
factum-mcp-server
    ↕ Rust API
factum-rt (store, query, arbitration, permissions)
    ↕
factum-core (parser, types, serialization)
```

The server maintains an in-memory knowledge store. Each session starts fresh.
For persistent storage (RocksDB backend), see the roadmap (M2).

## Morpheme negotiation

When the client declares `capabilities.factum` in the initialize request, the
server returns a morpheme table (24 seed morphemes with ID/name/kind). This
enables compact form encoding — morpheme names are replaced by u32 indices,
saving bytes in transport.

If the client does not declare `factum` capability, the server omits the
morpheme table and all responses use string names (graceful degradation).

## Troubleshooting

**Server not responding**: Make sure you're piping newline-delimited JSON. The
server reads one line at a time. Empty lines are ignored.

**Parse error on insert**: The Factum-F source must be valid S-expression.
Check for balanced parentheses. Common mistakes:
- Missing `:pred` field
- Named args before positional args
- Unbalanced parentheses

**Query returns 0 results**: Check the `min_confidence` threshold (default 0.0)
and the permission context. The server uses `public` permission by default —
nodes with `:perm confidential` or higher will not be visible.

## Next steps

- Read the [LLM authoring guide](authoring-for-llms.md) for syntax details
- Read the [design rationale](design-rationale.md) for architectural decisions
- Read the [technical white paper](whitepaper-zh.md) (Chinese) for full overview
- Check [good first issues](GOOD_FIRST_ISSUES.md) for contribution opportunities
- See the [roadmap](../ROADMAP.md) for what's planned next
