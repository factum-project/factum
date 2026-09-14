# factum-core

Core data model, parser, and serialization for [Factum](https://github.com/factum-project/factum) — a structured knowledge representation language designed as a native format for LLMs.

## Features

- **Node 7-tuple**: id, predicate, validity, provenance, confidence, authority, permissions
- **Lossless numerics**: all numbers use `Dec(i128, u8)` — zero floating-point error
- **5-level provenance**: Verbatim / Summary / Extracted / Derived / Asserted
- **Full parenthesization**: parse-safe for LLM generation
- **100% syntactic round-trip**: parse → serialize → parse is idempotent

See the [main project README](https://github.com/factum-project/factum) for the full language specification and architecture.

- MCP Registry name: `mcp-name: io.github.factum-project/factum`
