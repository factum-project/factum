# factum-rt

Runtime for [Factum](https://github.com/factum-project/factum) — in-memory store, indexing, query, arbitration, and permissions.

## Features

- **In-memory store** (default) + **RocksDB persistence backend** (`--features rocksdb`)
- 5 column families, WriteBatch atomic writes, crash-durable
- Index-level permissions (no post-query filtering — prevents aggregate leakage)
- Conflict arbitration: LatestWins / HighestAuthority / Unanimous
- Cascade retraction: derived nodes auto-invalidate when upstream sources are retracted

See the [main project README](https://github.com/factum-project/factum) for the full language specification and architecture.

- MCP Registry name: `mcp-name: io.github.factum-project/factum`
