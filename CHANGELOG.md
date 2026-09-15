# Changelog

All notable changes to Factum will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] — 2026-09-15

### Changed
- **Repositioning**: "A Native Knowledge Language for LLMs" → "Auditable Memory for AI Agents". The technology is unchanged — this is a messaging change that aligns Factum with the agent memory market (Mem0, Zep, Letta) where its features (provenance, cascade retraction, conflict refusal) directly solve known pain points. "Knowledge language" remains in design docs as the mechanism description.
- **README rewritten**: New首屏 leads with agent memory positioning, includes双向对比表 (unique to Factum vs not-yet-in-Factum), and STALE benchmark citation (arxiv.org/abs/2605.06527).
- **ROADMAP reordered**: Agent memory core items (RocksDB, MCP, cascade retraction) moved to top of M2; corpus converters and verifiers moved to lower priority. No new items added — only priority reordering. "No agent framework" and "no embedding search" added to "Not on the Roadmap" section.
- **server.json**: Description updated to "Auditable memory for AI agents: provenance, cascade retraction, conflict refusal" (within 100-char Registry limit).
- Version: `0.1.0` → `0.1.1`

### Fixed
- **LatestWins validity tiebreak**: `ConflictPolicy::LatestWins` previously compared only authority, ignoring the promised validity start tiebreak. Now correctly sorts by authority desc, then validity start desc (most recent wins). `Forever` is treated as the least recent. When authority AND validity start are both tied, the result is marked `Ambiguous`.
- **ConflictPolicy::Custom silent guess**: `Custom` previously returned the first result silently (`group.into_iter().next().unwrap()`), violating the "We refuse to answer rather than guess" principle. Now sets `ambiguous = true` and returns no result for multi-node groups. Single-node groups still pass through normally.
- **ArithmeticVerifier → DecimalRangeVerifier**: Renamed to match actual behavior. The verifier only checks decimal scale (≤38) and digit count (≤38), not arithmetic consistency. Doc comment updated to explicitly state this limitation and point to `SolverVerifier` / `LeanVerifier` for future arithmetic checks.
- **MCP serverInfo version hardcoded**: `handler.rs` had `"0.1.0"` hardcoded instead of using `env!("CARGO_PKG_VERSION")`. Now correctly reports the crate version (0.1.1).
- **Lexer multi-dot number silent corruption**: `0.1.1`, `192.168.1.1` and similar multi-dot numbers were silently split into multiple tokens (`0.1` + `@.1`), corrupting the knowledge graph without any error. Now produces a clear parse error guiding users to use string quotes (e.g., `"0.1.1"`).

### Added
- **`factum_lookup` MCP tool**: New tool that looks up all knowledge about a specific entity using the `by_entity` index. Takes an entity name (with or without `@` prefix) and optional `min_confidence` filter. Returns all active public nodes where the entity appears in predicate arguments. This is the #1 agent memory use case — "what do I know about X?" — that previously required knowing the exact predicate.
- **`factum_insert_batch` MCP tool**: New tool for atomic multi-node insertion. Takes an array of node strings (max 100). If any node fails parsing, the entire batch is rejected (no partial insert). Uses `FactumStore::insert_batch()` which also checks for duplicates and verifier failures atomically. 10x more efficient than calling `factum_insert` repeatedly for initial knowledge base loading.
- **String literal guidelines in authoring guide**: New section in `docs/authoring-for-llms.md` documenting when values must be wrapped in double quotes — version numbers (`0.1.1`), URLs (`https://...`), IP addresses, file paths with colons, email addresses, and free-text descriptions. Discovered during first real-world self-use of Factum as agent memory.

### Added
- **MCP server RocksDB persistence support**: `factum-mcp-server` now accepts `--db-path <PATH>` CLI argument to enable RocksDB persistence. Requires building with `--features rocksdb`. Without the flag, the server defaults to in-memory mode (backwards compatible). The `factum-mcp` crate now has an optional `rocksdb` feature that forwards to `factum-rt/rocksdb`.
- **MCP server CLI help**: `--help` / `-h` prints usage; `--in-memory` explicitly forces default mode; unknown args produce clear error messages instead of being silently ignored.
- **Morpheme vocabulary expanded from 24 to 200+ seeds**: Added 4 new `MorphemeKind` variants (Status, Action, Attribute, Classification) to the existing 5 (Entity, Relation, Quantifier, Modal, Temporal). The 200+ seed morphemes now cover: 30 entity types, 100+ relations (organizational, people, spatial, financial, product/project, version control, document/knowledge, agent memory, permission, cause/effect), 6 quantifiers, 6 modals, 8 temporal operators, 20 status states, 20 actions, 20 attributes, and 20 classification tags. Sufficient for real agent memory use cases.
- **Confidence calibration guide** (`docs/confidence-calibration.md`): Detailed tables mapping source types, extraction methods, and knowledge categories to recommended `confidence` and `authority` values. Includes practical examples, common mistakes, and query-time threshold guidance.

### Added
- **RocksDB persistence backend** (feature `rocksdb`): `RocksDBBackend` implements `StorageBackend` trait with 5 column families (nodes + by_entity + by_pred + by_src + by_perm). Uses bincode for Node serialization, RocksDB's native WAL for durability, and `WriteBatch` for atomic multi-key writes. `FactumStore::with_rocksdb(path, registry)` constructor opens or creates a persistent database. `deps_rev` and `by_validity` indices are rebuilt in-memory on startup from persisted node data.
- **StorageBackend trait abstraction**: `FactumStore` now holds `Arc<dyn StorageBackend>` instead of direct HashMap fields. Two implementations: `InMemoryBackend` (default, identical to v0.1 behavior) and `RocksDBBackend` (feature-gated). Zero breaking change to public API — all existing methods and constructors work unchanged.
- **Serde derives on all core types**: `Node`, `Predicate`, `Term`, `Literal`, `Provenance`, `Validity`, `NodeId`, `EntityId`, `DocId`, and all other core types now derive `serde::Serialize` and `serde::Deserialize` (or have manual impls for `Arc<str>` newtypes). Enables bincode serialization for RocksDB persistence.
- **preferred_form negotiation** (spec/compact-form.md §8): `McpHandler` reads `capabilities.factum.preferred_form` during initialize. When `"canonical"`, `factum_query` results return as S-expression text (−62% tokens vs JSON for LLM context). When absent or `"compact"`, defaults to compact JSON. Invalid values safely fall back to compact.
- **Store ↔ SubscriptionManager integration**: `FactumStore` now holds a `SubscriptionManager` instance. `insert()`, `insert_batch()`, and `retract()` call `notify_insert()` / `notify_retract()` respectively. Public `subscriptions()` accessor for creating subscriptions.
- **Store ↔ VerifierRegistry integration**: `FactumStore` now holds an optional `VerifierRegistry`. `enable_verifiers()` / `set_verifiers()` / `disable_verifiers()` methods. `insert()` and `insert_batch()` verify before storage when enabled. Off by default — opt-in, no breaking change. Batch verification is atomic: one bad node rejects the entire batch.
- **MCP stdio transport**: `factum-mcp-server` binary reads newline-delimited JSON-RPC from stdin, writes responses to stdout. Verified end-to-end: initialize → insert → query flow works with real stdin/stdout.
- **Real tokenizer measurement** (issue #9 ✅): `tiktoken-rs` (o200k_base / GPT-4o) replaces heuristic estimator. Real token counts: canonical 238 tokens (−62% vs JSON), compact 290 tokens (−53% vs JSON), verbose JSON 623 tokens. Heuristic estimator retained as fallback, marked with 20.7% / 43.7% estimation error.
- **Getting Started guide** (`docs/getting-started-mcp.md`): step-by-step MCP server setup, Claude Code / Cursor configuration, example insert + query flows, troubleshooting.
- **Interactive visualization page** (`docs/site/index.html`): GitHub Pages-ready HTML with animated syntax parsing, interactive 7-tuple explorer, 6-step query pipeline stepper, token efficiency chart, MCP architecture diagram, and roadmap timeline.
- **Chinese technical white paper** (`docs/whitepaper-zh.md`): 20 sections + 4 appendices covering the complete Factum architecture.

### Changed
- **Token efficiency table updated with real o200k_base measurements**: compact −53% tokens (was −12% estimated), canonical −62% tokens (was −66% estimated). Key finding confirmed: canonical form is more token-efficient than compact form.
- **Form-positioning decision confirmed** (spec/compact-form.md §8): Real tokenizer data confirms canonical beats compact on tokens. `capabilities.factum.preferred_form` negotiation implemented — serves canonical to LLM clients, reposition compact as storage/service-to-service format.
- **Byte efficiency table updated**: compact 643 bytes (−68%), canonical 650 bytes (−67%), JSON 1994 bytes (baseline), Markdown 420 bytes (−79%). Previous numbers used a smaller JSON baseline.
- **ROADMAP M2**: issue #9 marked ✅, form-positioning decision updated from "pending" to "confirmed".

### Renamed
- **Renamed from AXON to Factum** for namespace clarity. All crate names, module names, types, URLs, and documentation updated. The language specification name changed from AXON-F to Factum-F. No functional changes.

### Added
- Conformance test vectors: canonical full-text comparison + error taxonomy + manifest
- Error class identifiers embedded in parser/lexer error messages for conformance matching
- Compact form wire format spec as standalone document (`spec/compact-form.md`, compact-form v0.1-draft)
- Literal disambiguation rule for compact form (string `"?foo"` vs Var `?foo`)
- MCP extension compliance: `factum_morphemes` sent only to clients declaring `capabilities.factum`; graceful degradation to string names
- Factum-aware client detection via MCP `capabilities.factum` capability declaration (standard MCP negotiation pattern)
- Compact form fallback behavior specification (morpheme negotiation failure → string name form)
- CompactValidity semantics: omitted `u` = open-ended window (distinct from Forever)
- Compact form conformance vectors (`spec/conformance/compact_basic.json`) — 3 core vectors: string disambiguation (?/@), open-ended validity, round-trip identity
- "Relationship to other formats" section in README (RDF, JSON-LD, CUE, Datalog, Markdown, JSON)
- "Identifier Character Set" section in README (ASCII-only for v0.1-alpha)
- `charset_policy` field in conformance manifest.json
- `docs/design-rationale.md` (English design decision rationale, replaces internal Chinese EVALUATION.md)
- CI badge in README
- cargo-fuzz CI integration (10 min short runs + weekly long runs)
- Token efficiency table in README (heuristic estimate, ±15% of o200k_base; real measurement tracked in issue #9)
- LLM authoring guide (`docs/authoring-for-llms.md`) — error self-correction patterns, few-shot templates, query result interpretation, provenance selection
- Form-positioning decision item in ROADMAP M2 (pending issue #9 real tokenizer data)
- Form-positioning spec section in `spec/compact-form.md` §8 (pending decision, triggered by canonical −66% vs compact −12% token finding)
- Token efficiency as 2nd argument for S-expressions in `docs/design-rationale.md` §1 (quantitative validation that canonical form is most token-efficient)
- Good first issue #9: replace heuristic token estimator with real tokenizer (`tiktoken-rs`)
- Clippy: 0 warnings (`cargo clippy --all-targets`)

### Changed
- LICENSE copyright: AXON Project → Factum Project Contributors
- Placeholder emails replaced: `security@factum.dev` / `conduct@factum.dev` with GitHub Security Advisory fallback
- GitHub org URL unified to `factum-project/factum` across all docs
- ROADMAP M0: split `factum-mcp` into protocol/handler (✅) and transport (📋) — accurate reflection of implementation state
- README "What's Implemented" table: factum-mcp now shows protocol+handler ✅, transport not verified
- Benchmark numbers unified: all percentages use pretty-JSON-with-same-metadata as baseline (compact = −76%)
- Benchmark table baseline row: "Same data as Markdown + no metadata" → "Same 7-tuple metadata in verbose JSON encoding" (fixes self-contradiction)
- Version: `0.1.0` → `0.1.0` (no change; unified to `0.1.0`)
- README status line: `v0.1.0-alpha` → `v0.1.0` (match workspace version)
- All Huawei-specific examples replaced with generic names: @ACME-CORP / @FOUNDER-1 / @ACME-SUB / @ACME-HQ
- Factum-aware client detection: changed from `clientInfo.factum_aware` to standard MCP `capabilities.factum` capability declaration
- CONTRIBUTING testing table: morpheme mechanism updated to reflect current `seed_morphemes()` (pre-M2) vs future `morphemes.toml` (M2+)
- Good first issue #2: removed Unicode test vectors (charset decision deferred), added compact form edge cases
- Good first issue #4: upgraded from "key fields match" to full round-trip + canonical equivalence
- Good first issue #8: added stdio transport verification prerequisite
- README Three-Layer Vision table: "token efficiency measured" → "estimated (heuristic; real measurement tracked in issue #9)" (strict claim-evidence alignment)
- ROADMAP M1: added completed items (authoring guide, token table, issue #9, clippy 0 warnings, issue/PR templates)
- ROADMAP M2 Benchmarks: added P0 item for real tokenizer replacement + form-positioning decision
- Authoring guide: "Validate before submitting" rewritten for LLM audience (LLMs submit via factum_insert and self-correct on error, not parse locally)
- Authoring guide: Template 3 deps fixed (`[n001 n002]` → `[n001]` — n002 unrelated to subsidiary-of derivation)
- Authoring guide: added "Interpreting query results" section (compact form number tags, term disambiguation, Ambiguous result handling)
- design-rationale.md §1: S-expression token efficiency upgraded from design intuition to measured finding (−66% canonical vs −12% compact)
- `WalEntry::Insert(Node)` → `WalEntry::Insert(Box<Node>)` (clippy large_enum_variant fix)
- .gitignore: removed `Cargo.lock` (workspace has binary crate `factum-demo`, lock file should be committed)

### Removed
- `EVALUATION.md` (internal Chinese risk assessment — not appropriate for public repo)

## [0.1.0-alpha] — 2026-09-08

### Added
- **factum-core**: Node 7-tuple data model (id, predicate, validity, provenance, confidence, authority, permissions) + deps + status
- **factum-core**: S-expression lexer with full token types (symbols, variables, keywords, strings, numbers, dates, durations, booleans, URIs, entity references)
- **factum-core**: Hand-written recursive-descent parser with full parenthesization for parse uniqueness
- **factum-core**: Canonical serialization (fixed field order, minimal Dec representation) + compact serialization (JSON with morpheme index + numeric tags)
- **factum-core**: Morpheme registry with 200+ seed morphemes across 9 kinds (Entity, Relation, Quantifier, Modal, Temporal, Status, Action, Attribute, Classification)
- **factum-core**: Parser depth limit (`MAX_PARSE_DEPTH = 128`) and lexer token count limit (`MAX_TOKENS = 1M`) for DoS protection
- **factum-core**: cargo-fuzz targets for parser, serialize round-trip, and lexer
- **factum-rt**: In-memory store with 6 secondary indices (by_entity, by_pred, by_src, by_perm, deps_rev, by_validity)
- **factum-rt**: Query engine with variable binding and pattern matching
- **factum-rt**: Conflict arbitration (LatestWins, HighestAuthority, Unanimous, Custom) with Ambiguous refusal
- **factum-rt**: Index-level permission filtering (no post-query filtering — prevents aggregate leakage)
- **factum-rt**: Verifier framework with SchemaVerifier (morpheme signature checking) and DecimalRangeVerifier (decimal range checking)
- **factum-rt**: Subscription manager with pattern-matched event notification
- **factum-rt**: Cascade retraction propagation via reverse dependency graph
- **factum-rt**: WAL (write-ahead log) for event replay
- **factum-mcp**: JSON-RPC 2.0 protocol implementation (MCP 2025-06-18)
- **factum-mcp**: Five MCP tools: `factum_query`, `factum_lookup`, `factum_insert`, `factum_insert_batch`, `factum_retract`
- **factum-mcp**: Resource URI pattern `factum://nodes/{id}`
- **factum-mcp**: Morpheme table negotiation during `initialize` handshake
- **factum-bench**: Syntax round-trip benchmark (1000 nodes, 100%)
- **factum-bench**: Token efficiency comparison (Factum canonical vs compact vs Markdown vs JSON)
- **factum-bench**: Query performance benchmark
- **factum-demo**: End-to-end demonstration binary
- **CI**: GitHub Actions workflow (test + clippy + fuzz + gitleaks)
- All tests passing across all crates

### Known Limitations
- Storage defaults to in-memory; RocksDB persistence available via `--features rocksdb` (no MVCC yet)
- Morpheme vocabulary is 200+ seeds (target: 200-500)
- MCP transport: stdio verified, HTTP not started
- No real MCP host integration testing (Claude Code, Cursor)
- No corpus converters (Wikidata, Mathlib)
- No Lean/Z3 verifier integration
- Semantic round-trip (factum-l latent projection) not started
- No Inspector (visual debugger)
- Identifier charset is ASCII-only (Unicode deferred to future spec version)
- Verifiers are opt-in (off by default); subscription manager does not persist events across sessions
