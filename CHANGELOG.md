# Changelog

All notable changes to Factum will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed (critical)
- **Arbitration grouping bug (architecture-level)**: `group_by_bindings` grouped by variable binding values, so conflicting values for the same attribute landed in separate groups — arbitration never engaged for value conflicts (the most common conflict type). Fixed: `arbitrate()` now accepts an optional `&Predicate` query pattern; `group_by_pattern()` groups by head + ground argument positions, excluding variable positions. WeightedVote and Unanimous now correctly detect and resolve value conflicts (e.g., `(status @X active)` vs `(status @X inactive)`). 4 end-to-end regression tests added in query.rs.
- **Corroboration rejected as error**: `factum_assert` returned `AlreadyExists` error when a second agent asserted the same fact (same content-addressed ID). This discarded the most valuable multi-agent signal — independent agreement. Fixed: `AlreadyExists` now checks if the existing principal differs from the new one. Different principal → returns `corroborated` (success, not error). Same principal → returns `duplicate` (idempotent success). 4 new tests.
- **Parameter name silently ignored**: `FactumAssertParams` lacked `deny_unknown_fields`, so `"conf"` (wrong field name) was silently dropped and confidence defaulted to 0.60. Fixed: added `#[serde(deny_unknown_fields)]` and `#[serde(alias = "conf")]` on `confidence` field. Unknown fields now cause a parse error. 2 new tests.
- **Cascade limit naming**: `max_cascade_depth` was actually a node count limit, not a recursion depth limit. The depth check in `retract_recursive` was unreachable dead code (mathematically provable). `StoreError::CascadeLimitExceeded` was defined but never constructed. Fixed: renamed to `retract_with_limit(id, max_nodes)` / `max_cascade_nodes`. Removed dead `CascadeLimitExceeded` variant. Removed unreachable depth check. Test names corrected.
- **Multi-agent demo script non-runnable**: Demo used wrong parameter name (`"text"` instead of `"predicate"`, `"conf"` instead of `"confidence"`), placeholder IDs (`auto-aaa-001`), and didn't mention corroboration behavior. Rewritten with correct parameter names, real ID format explanation, corroboration step, and `max_cascade_nodes` parameter.
- **Architecture diagram misleading**: multi-agent-usage.md showed 3 clients connecting to 1 stdio server, but stdio means each client gets its own process. Rewritten with accurate diagram showing separate processes.
- **Read-only mode fiction**: multi-agent-usage.md claimed agents could "start their own Factum process pointing at the same RocksDB path in read-only mode" — no read-only mode exists in code. RocksDB acquires an exclusive lock on open. Fixed: workaround section now describes coordinator pattern, sequential writing, and export/import.
- **FactumAssertParams rustdoc outdated**: Comment said "(default: 1.0)" but default was changed to provenance-based in 0235c48. Fixed.

### Added
- **Multi-agent demo integration tests** (`crates/factum-mcp/tests/multi_agent_demo.rs`): 8 end-to-end tests verifying every step of the demo script. Tests: conflicting assertions succeed, corroboration returns success, same-principal idempotent, WeightedVote resolves conflict, equal weights ambiguous, cascade retraction, conf alias, unknown fields rejected. Serves as CI gate — if demo claims a behavior, this test must pass.
- **Arbitration pattern-aware grouping**: `arbitrate()` now accepts `Option<&Predicate>` query pattern. When provided, `group_by_pattern()` groups by ground positions (head + non-variable args), ensuring conflicting values for the same variable position land in the same group. `sub_group_by_predicate()` within LatestWins/HighestAuthority further separates different concrete predicates (different entities) from true conflicts (same predicate, different metadata).
- **Corroboration detection in factum_assert**: AlreadyExists now returns `corroborated` (different principal) or `duplicate` (same principal) instead of an error. Response includes `corroborated_by` and `your_principal` fields.
- **serde `deny_unknown_fields` + `conf` alias**: `FactumAssertParams` now rejects unknown fields and accepts `conf` as an alias for `confidence`.
- **HTTP transport design document** (`docs/http-transport-design.md`): Design doc for M2 Streamable HTTP transport. Covers axum framework choice, API surface (POST/GET/DELETE /mcp), concurrency model (Phase 1: RwLock, Phase 2: MVCC), session management, security considerations, and testing strategy.
- **Large-scale deployment guidance**: New "Scaling to 10+ Agents" section in `docs/multi-agent-usage.md` covering trust tiers, cascade node limit, garbage metadata defense (min_conf + permission isolation + WeightedVote), and write coordination patterns.
- **WeightedVote conflict policy**: New `ConflictPolicy::WeightedVote { weights }` variant for multi-agent conflict resolution. Groups conflicting nodes by predicate canonical form, sums per-principal weights, and resolves if a group exceeds 50% of total weight. Returns `Ambiguous` when no majority exists (refuses to answer). MCP `factum_query` tool now accepts `policy: "weighted"` with optional `agent_weights` parameter. 5 tests covering majority win, no-majority ambiguity, all-agree, unknown-principal default weight, and single-node passthrough.
- **Multi-agent usage guide** (`docs/multi-agent-usage.md`): Comprehensive guide for using Factum as a shared knowledge base for multiple agents. Covers architecture limitations, 5 core capabilities (provenance, permissions, dedup, cascade retraction, weighted voting), 3 conflict scenarios, best practices, and explicit "what Factum does NOT do" (agent orchestration, messaging, identity management).
- **M3 reliability table extended to `(provenance, model, principal)`**: The Empirical Reliability Table design now includes a `principal` dimension, enabling per-agent reliability tracking in multi-agent scenarios. Updated in ROADMAP.md, calibration.rs doc comments, and confidence-calibration-research.md.

### Changed
- `ConflictPolicy` enum: removed `Copy` derive (needed for `WeightedVote`'s `HashMap`). All usage sites use `Clone` or references — no breaking changes.
- README: conflict arbitration now lists 4 policies (added WeightedVote). Links to multi-agent usage guide.
- `factum_query` MCP tool schema: `policy` enum adds `"weighted"`. New `agent_weights` parameter (JSON object of principal→weight).

### Fixed
- **f32 confidence/authority serialization (ISSUES #5)**: `format!("{}", 1.0f32)` produces `"1"` instead of `"1.0"`, causing round-trip parsing issues. Added `format_f32_with_decimal()` helper in `serialize.rs` that ensures at least one decimal place. Applied to `:conf` and `:auth` canonical serialization. 3 regression tests added.
- **factum_lookup tool description (ISSUES #11)**: Tool description now explicitly states it uses exact entity name matching and suggests `factum_search` for partial matching. Updated `getting-started-mcp.md` tool list accordingly.
- **Binary rebuild troubleshooting (ISSUES #12)**: Added troubleshooting note in `getting-started-mcp.md` about MCP client schema caching — users must restart client or re-trust connector after rebuilding the binary.
- **Mixed-era knowledge base guidance (ISSUES #13)**: New "Mixed-Era Knowledge Bases" section in `confidence-calibration.md` documenting the old-node-conf-inflation vs new-node-honesty paradox and providing short/medium/long-term mitigation strategies.
- **authoring-for-llms.md outdated defaults**: Updated `:conf` default documentation from `1.0` to provenance-based defaults. Template 1 confidence reduced from `0.99` to `0.95` with band-clipping note. Best practice #2 rewritten to reflect automatic provenance-based defaults.

### Added
- **Confidence calibration research document** (`docs/confidence-calibration-research.md`): Comprehensive literature review of 7 academic papers (2023–2025) on LLM confidence calibration. Covers verbalized confidence (Tian et al. 2023), sample consistency (Lyu et al. AAAI 2025), clinical LLM overconfidence (JMIR 2025, r=−0.40), perturbed representation stability (CCPS, EMNLP 2025), knowledge graph trust tiers (Gold/Silver/Bronze/Quarantine), Source Provenance Score (6-component composite), and STALE benchmark (55.2% best accuracy). Synthesizes findings into actionable recommendation: provenance-based default confidence mapping.
- **Confidence calibration research v0.3 (post-review)**: Revised after 20-round expert review. Key changes: (1) Framed point values as "policy constants" not "scientific conclusions" — literature supports ordering, not point values. (2) Added band clipping mechanism: each provenance type has an allowed agent self-assessment range; exceeding requires corroboration. (3) Replaced Derived "inherit" with multiplicative decay: `conf_derived = min(active_deps' conf) × rule_reliability` (default 0.95, verified rules = 1.0). (4) Added Empirical Reliability Table as M3 strategic upgrade: `(provenance × model)` historical accuracy replaces policy constants once N≥10 data points exist. (5) Added corroboration counting with `(principal, model)` deduplication. (6) Audited `min_conf` usage: no hardcoded thresholds in code; documentation examples updated with interaction note.

### Changed
- **`confidence-calibration.md` updated to v0.3**: "Mistake 1" section now includes band clipping table with allowed agent ranges, Derived decay formula, and min_conf interaction note. Provenance → default confidence table reframed as "policy constants." Version bumped to v0.3.

### Fixed
- **SchemaVerifier arity check broken for named/optional params**: The verifier only counted `node.predicate.args.len()` (positional args), completely ignoring `node.predicate.named`. Signatures with optional params (`?` suffix) or named params would fail validation when used as documented. Now counts `args.len() + named.len()` and properly handles `?` optional suffix in signature strings. Validation rule: `required_params <= (args + named) <= total_params`. Added 4 regression tests: named args counted, optional param omitted, optional param provided, too many args.
- **Arbitration API behavior undocumented**: `HighestAuthority` returns a candidate on tie (with `ambiguous = true`), while `Unanimous` returns nothing on disagreement (only `ambiguous = true`). This asymmetry is intentional but was not documented. Added module-level doc table explaining the behavior difference, and strengthened two tests to assert `results.len()` (non-empty for HighestAuthority, empty for Unanimous).
- **Upsert non-atomic window undocumented**: `factum_upsert` performs insert and retract as separate operations. If insert succeeds but retract fails, both nodes exist (returned as `action: "partial"`). This is not data loss, but was not documented. Added doc comment explaining the non-atomic nature and pointing to future WriteBatch API for true atomicity.

### Added
- **`factum_assert` MCP tool**: New tool that accepts only a predicate S-expression (e.g. `(version @FACTUM "0.1.3")`), auto-generates a content-based node ID (`auto-` + 12 hex chars of hash), and assigns default provenance (`Asserted { by: "system" }`). Same content → same ID → second insert fails with `AlreadyExists` (prevents accidental duplicates). Optional `by` and `confidence` parameters for customization. Reduces typical insert from 60+ chars to ~30 chars. Solves dogfooding issues #8 (verbose syntax) and #10 (node ID collision risk).
- **`pub fn canonical_predicate()`**: Previously private `canonical_predicate` function in `serialize.rs` is now public, enabling external callers to serialize predicates independently.
- **28 handler unit tests**: Comprehensive tests for all 8 MCP tools — factum_insert (3: success, duplicate, parse error), factum_retract (2: success, not_found), factum_lookup (2: match, no_match), factum_insert_batch (2: success, parse_error_rejects_all), factum_upsert (3: 0_match, 1_match, multi_ambiguous), factum_assert (4: success, duplicate, parse_error, custom_params), factum_search (5: keyword, predicates, stats, invalid_mode, missing_keyword), store_error_to_jsonrpc (3: NotFound, AlreadyExists, Storage), generate_content_id (2: deterministic, different_content), listChanged declared (1). Handler test count: 14 → 42.

### Fixed
- **`by_validity` index unused**: `lookup_valid_at()` previously called `iter_nodes()` loading all nodes from the backend, despite a `by_validity` BTreeMap index being built in `rebuild_indices()`. Now uses BTreeMap range query `range(..=(t_ts, i64::MAX))` to select only candidate entries whose `from_ts <= t`, then filters on `until_ts` and Active status. Added `update_by_validity()` called during `insert()` and `insert_batch()` to keep the index in sync with new nodes.
- **StoreError → MCP error code mapping**: `AlreadyExists`, `NotFound`, `PermissionDenied`, and `InvalidNode` were all mapped to MCP `internal` error (-32603), making it impossible for LLM clients to distinguish "node already exists" from a server crash. Now correctly mapped to `invalid_params` (-32602) via new `store_error_to_jsonrpc()` helper. Only `Storage` errors remain as `internal` (-32603).
- **Search double serialization**: `factum_search` keyword mode called `serialize::canonical(n)` twice per node — once in `filter()` to check the keyword match, and again in `map()` to collect the result. Changed to `map().filter().take()` pipeline so each node is serialized only once.
- **`tools/listChanged` not declared**: Server capabilities declared `listChanged: None` for tools, causing MCP clients (including the WorkBuddy connector) to cache `tools/list` at startup and never re-query after server version updates. Now declares `listChanged: Some(true)` — clients will re-query tools on reconnect.
- **Tautology test**: `test_parse_error_named_before_positional` had `assert!(result.is_err() || result.is_ok())` which is always true. Fixed to `assert!(result.is_err())`.
- **factum-mcp/README.md outdated**: Listed only 3 tools instead of 8. Updated to list all 8 tools with descriptions.
- **factum-mcp/lib.rs module doc**: Listed only 3 tools. Updated to "8 tools".

### Changed
- MCP tool count: 7 → 8
- Handler unit test count: 14 → 42
- Total test count: 115 → 147

### Added
- **`factum_upsert` MCP tool**: New tool for update-or-insert. Finds active nodes matching entity + predicate, then: 0 matches → plain insert; 1 match → insert new + retract old; 2+ matches → returns Ambiguous (refuses to guess). Insert-first ordering ensures no data loss on partial failure. Reduces 3-step update (query → retract → insert) to a single call.
- **`factum_search` MCP tool**: New tool with three modes — keyword (case-insensitive substring search over canonical text, max 200 results), predicates (list all distinct predicate heads with counts), stats (total/active/retracted node counts + per-predicate breakdown). Helps agents answer "what do I know?" and "what predicates exist?" without knowing exact patterns.

### Changed
- Version: `0.1.2` → `0.1.3` (0.1.2 was already published to crates.io before these two new tools were added)
- MCP tool count: 5 → 7

## [0.1.2] — 2026-09-15

### Fixed
- **Lexer multi-dot number silent corruption**: `0.1.1`, `192.168.1.1` and similar multi-dot numbers were silently split into multiple tokens (`0.1` + `@.1`), corrupting the knowledge graph without any error. Now produces a clear parse error guiding users to use string quotes (e.g., `"0.1.1"`). Discovered during first real-world self-use of Factum as agent memory.

### Added
- **`factum_lookup` MCP tool**: New tool that looks up all knowledge about a specific entity using the `by_entity` index. Takes an entity name (with or without `@` prefix) and optional `min_confidence` filter. Returns all active public nodes where the entity appears in predicate arguments. This is the #1 agent memory use case — "what do I know about X?" — that previously required knowing the exact predicate.
- **`factum_insert_batch` MCP tool**: New tool for atomic multi-node insertion. Takes an array of node strings (max 100). If any node fails parsing, the entire batch is rejected (no partial insert). Uses `FactumStore::insert_batch()` which also checks for duplicates and verifier failures atomically. 10x more efficient than calling `factum_insert` repeatedly for initial knowledge base loading.
- **`factum_upsert` MCP tool**: New tool for update-or-insert. Finds active nodes matching entity + predicate, then: 0 matches → plain insert; 1 match → insert new + retract old; 2+ matches → returns Ambiguous (refuses to guess). Insert-first ordering ensures no data loss on partial failure. Reduces 3-step update (query → retract → insert) to a single call.
- **`factum_search` MCP tool**: New tool with three modes — keyword (case-insensitive substring search over canonical text, max 200 results), predicates (list all distinct predicate heads with counts), stats (total/active/retracted node counts + per-predicate breakdown). Helps agents answer "what do I know?" and "what predicates exist?" without knowing exact patterns.
- **String literal guidelines in authoring guide**: New section in `docs/authoring-for-llms.md` documenting when values must be wrapped in double quotes — version numbers (`0.1.1`), URLs (`https://...`), IP addresses, file paths with colons, email addresses, and free-text descriptions. Discovered during first real-world self-use of Factum as agent memory.

### Changed
- Version: `0.1.1` → `0.1.2` (0.1.1 was already published to crates.io before these improvements were made)

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
