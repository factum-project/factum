# Getting Started with Factum MCP

Run Factum as a local MCP server and use it from Claude Code or Cursor.
The client starts `factum-mcp-server` as a subprocess and exchanges
newline-delimited JSON-RPC over stdin/stdout. No HTTP service is needed.

## Prerequisites and build

Install a current stable Rust toolchain and Git, then build the server:

```bash
git clone https://github.com/factum-project/factum.git
cd factum
cargo build --release -p factum-mcp --bin factum-mcp-server
```

The executable is `target/release/factum-mcp-server` (with `.exe` on Windows).
Client configurations must point to the executable on your own machine.

### In-memory vs persistent (RocksDB)

The default build runs **in-memory**: all data is lost when the server process
exits. For agent memory that must survive restarts, build with the `rocksdb`
feature and pass `--db-path`:

```bash
cargo build --release -p factum-mcp --features rocksdb --bin factum-mcp-server
```

Then use `--db-path` when registering the server:

```bash
claude mcp add --transport stdio --scope local factum -- \
  "$(pwd)/target/release/factum-mcp-server" --db-path ~/.factum/store
```

The directory is created automatically on first run. Data persists across
process restarts — kill the server, restart it, and previously inserted nodes
are still queryable.

## Configure Claude Code

From your Factum checkout, register the executable for this local project:

```bash
claude mcp add --transport stdio --scope local factum -- "$(pwd)/target/release/factum-mcp-server"
claude mcp get factum
```

Start a new Claude Code session in the same directory and run `/mcp` to check
that `factum` is connected. The available tools are:

- `factum_query`: query matching nodes by predicate pattern.
- `factum_lookup`: look up all knowledge about a specific entity.
- `factum_insert`: insert a single knowledge node (full syntax with provenance, validity, etc.).
- `factum_insert_batch`: insert multiple nodes atomically (all-or-nothing).
- `factum_upsert`: update or insert a node (retract old matching node + insert new).
- `factum_assert`: assert a fact with minimal syntax (auto-generated node ID + default provenance).
- `factum_search`: search nodes by keyword, list predicates, or get stats.
- `factum_retract`: retract a node and its derived dependents.

To try the server without registering it permanently, save the following as
`factum.mcp.json`, replacing the executable path:

```json
{
  "mcpServers": {
    "factum": {
      "command": "/absolute/path/to/factum/target/release/factum-mcp-server",
      "args": []
    }
  }
}
```

Then launch a session with only this MCP configuration:

```bash
claude --mcp-config ./factum.mcp.json --strict-mcp-config
```

To remove a locally registered server, use `claude mcp remove --scope local factum`.

## Try a complete workflow

Ask Claude Code to perform these operations in order, using the actual tools:

> Insert this test node:
> `(node example001 :pred (located-in @QINGXI-FACTORY @SONGLAN-CITY) :valid forever :src (asserted "manual-test") :conf 0.95 :auth 0.8 :perm public :deps [])`.
> Query `(located-in @QINGXI-FACTORY ?city)` and report the returned location.
> Read the MCP resource `factum://nodes/example001` and check the same relation.
> Retract `example001`, then repeat the query to confirm there are no results.

Expected observations:

| Operation | Result |
|---|---|
| Insert | `Node inserted successfully` |
| Query | `count: 1`, with `@SONGLAN-CITY` in the canonical node |
| Read resource | `contents` contains the URI, `mimeType: text/plain`, and node text |
| Retract | `retracted: ["example001"]`, `count: 1` |
| Query again | `count: 0`, `nodes: []` |

The store is **in memory** by default. For persistence across sessions, build
with `--features rocksdb` and pass `--db-path`. A retracted node is retained
internally for auditing but is absent from normal queries, resource listings,
and resource reads. To repeat an insertion within one server process, use a
fresh node ID.

## Configure Cursor

In the target project's `.cursor/mcp.json`, add the same `mcpServers` object
shown above, with your executable's absolute path. Enable the server in Cursor's
MCP settings and use the same workflow.

## Verify directly over stdin/stdout

This uses the same executable and a complete initialization sequence. Each JSON
object must occupy one line. An initialization notification has no request ID
and receives no response.

```bash
cat <<'EOF' | ./target/release/factum-mcp-server
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"manual-check","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"factum_insert","arguments":{"node":"(node example001 :pred (located-in @QINGXI-FACTORY @SONGLAN-CITY) :valid forever :src (asserted \"manual-test\") :conf 0.95 :auth 0.8 :perm public :deps [])"}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"factum_query","arguments":{"query":"(located-in @QINGXI-FACTORY ?city)"}}}
{"jsonrpc":"2.0","id":5,"method":"resources/list","params":{}}
{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"factum://nodes/example001"}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"factum_retract","arguments":{"node_id":"example001"}}}
{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"factum_query","arguments":{"query":"(located-in @QINGXI-FACTORY ?city)"}}}
EOF
```

There should be eight response lines, with IDs 1 through 8. The final query
returns zero nodes. Query/retraction responses provide standard text content
and the same JSON object in `structuredContent`. For example, response 8 is:

```json
{
  "jsonrpc": "2.0",
  "id": 8,
  "result": {
    "content": [{"type": "text", "text": "{\"ambiguous\":false,\"count\":0,\"form\":\"canonical\",\"nodes\":[]}"}],
    "structuredContent": {"ambiguous": false, "count": 0, "form": "canonical", "nodes": []},
    "isError": false
  }
}
```

`resources/templates/list` exposes `factum://nodes/{id}`. Listings and reads
use the same public permission context as queries. Resource subscriptions and
change notifications are not implemented by the stdio bridge and are not
advertised as supported.

## Output formats

Ordinary MCP clients receive canonical Factum text with readable entity and
relation names. They need no custom vocabulary negotiation.

A custom client can declare `capabilities.factum` during initialization to
receive the 200+ entry seed morpheme table. Such clients default to compact output
and may explicitly choose `preferred_form: "canonical"` or `"compact"` inside
that capability. This extension is optional; Claude Code does not need to send it.

## Troubleshooting

- **Disconnected server:** verify the executable exists, has execute permission,
  and its configured path is absolute. Build it before launching the client.
- **Parse error on insert:** use valid Factum node syntax. Entity references use
  `@`, for example `(instance-of @ACME-CORP @organization)`. Start with the tested
  node above; use quoted source identifiers such as `(asserted "manual-test")`.
- **Zero query results:** check that insertion and query reach the same server
  process, that the node has `:perm public`, and that it has not been retracted.
  Time and minimum-confidence filters also affect query visibility.
- **Duplicate node ID:** use a different ID or start a fresh server process.
- **No output until newline:** stdio reads one JSON object per line. Empty lines
  are ignored; stdout is reserved for protocol responses.

## Tests and further reading

Run the regression tests, which launch the actual stdio server binary:

```bash
cargo test -p factum-mcp
```

For more detail, see the [LLM authoring guide](authoring-for-llms.md),
[design rationale](design-rationale.md), and [roadmap](../ROADMAP.md).
