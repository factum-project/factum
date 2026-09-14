# factum-mcp

MCP bridge for [Factum](https://github.com/factum-project/factum) — JSON-RPC 2.0 tools and resources over stdio transport.

## Tools

- `factum_insert` — insert a knowledge node with provenance and confidence
- `factum_query` — query matching nodes by predicate pattern
- `factum_retract` — retract a node and its derived dependents (audit trail retained)

## Quick start

```bash
cargo install factum-mcp

# Register with Claude Code
claude mcp add --transport stdio --scope local factum -- factum-mcp-server
```

See the [Getting Started guide](https://github.com/factum-project/factum/blob/main/docs/getting-started-mcp.md) for full setup instructions.

- MCP Registry name: `mcp-name: io.github.factum-project/factum`
