# factum-mcp

MCP bridge for [Factum](https://github.com/factum-project/factum) — JSON-RPC 2.0 tools and resources over stdio transport.

## Tools

- `factum_insert` — insert a knowledge node with provenance and confidence
- `factum_insert_batch` — insert multiple nodes atomically (all-or-nothing)
- `factum_assert` — assert a fact with minimal syntax (auto node ID + provenance)
- `factum_query` — query matching nodes by predicate pattern
- `factum_lookup` — look up all knowledge about a specific entity
- `factum_upsert` — update or insert a node (retract old + insert new)
- `factum_search` — search by keyword, list predicates, or get stats
- `factum_retract` — retract a node and its derived dependents (audit trail retained)

## Quick start

```bash
cargo install factum-mcp

# Register with Claude Code
claude mcp add --transport stdio --scope local factum -- factum-mcp-server
```

See the [Getting Started guide](https://github.com/factum-project/factum/blob/main/docs/getting-started-mcp.md) for full setup instructions.

- MCP Registry name: `mcp-name: io.github.factum-project/factum`
