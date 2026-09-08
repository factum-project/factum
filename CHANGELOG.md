# Changelog

All notable changes to Factum will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(with the `-alpha` suffix indicating pre-release status).

## [Unreleased]

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
- Version: `0.1.0` → `0.1.0-alpha.1` (semver pre-release format for crates.io compatibility)
- README status line: `v0.1.0-alpha` → `v0.1.0-alpha.1` (match workspace version)
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
- **factum-core**: Node 7-tuple data model (id, predicate, validity, provenance, confidence, authority, permissions, deps)
- **factum-core**: S-expression lexer with full token types (symbols, variables, keywords, strings, numbers, dates, durations, booleans, URIs, entity references)
- **factum-core**: Hand-written recursive-descent parser with full parenthesization for parse uniqueness
- **factum-core**: Canonical serialization (fixed field order, minimal Dec representation) + compact serialization (JSON with morpheme index + numeric tags)
- **factum-core**: Morpheme registry with 24 seed morphemes (Entity, Relation, Quantifier, Modal, Temporal)
- **factum-core**: Parser depth limit (`MAX_PARSE_DEPTH = 128`) and lexer token count limit (`MAX_TOKENS = 1M`) for DoS protection
- **factum-core**: cargo-fuzz targets for parser, serialize round-trip, and lexer
- **factum-rt**: In-memory store with 6 secondary indices (by_entity, by_pred, by_src, by_perm, deps_rev, by_validity)
- **factum-rt**: Query engine with variable binding and pattern matching
- **factum-rt**: Conflict arbitration (LatestWins, HighestAuthority, Unanimous) with Ambiguous refusal
- **factum-rt**: Index-level permission filtering (no post-query filtering — prevents aggregate leakage)
- **factum-rt**: Verifier framework with SchemaVerifier (morpheme signature checking) and ArithmeticVerifier (decimal range checking)
- **factum-rt**: Subscription manager with pattern-matched event notification
- **factum-rt**: Cascade retraction propagation via reverse dependency graph
- **factum-rt**: WAL (write-ahead log) for event replay
- **factum-mcp**: JSON-RPC 2.0 protocol implementation (MCP 2025-06-18)
- **factum-mcp**: Three MCP tools: `factum_query`, `factum_insert`, `factum_retract`
- **factum-mcp**: Resource URI pattern `factum://nodes/{id}`
- **factum-mcp**: Morpheme table negotiation during `initialize` handshake
- **factum-bench**: Syntax round-trip benchmark (1000 nodes, 100%)
- **factum-bench**: Token efficiency comparison (Factum canonical vs compact vs Markdown vs JSON)
- **factum-bench**: Query performance benchmark
- **factum-demo**: End-to-end demonstration binary
- **CI**: GitHub Actions workflow (test + clippy + fuzz + gitleaks)
- All tests passing across all crates

### Known Limitations
- Storage is in-memory only (RocksDB backend is planned)
- Morpheme vocabulary is 24 seeds (target: 200-500)
- MCP transport layer not implemented (protocol/handler only; stdio unverified, HTTP not started)
- No real MCP host integration testing (Claude Code, Cursor)
- No corpus converters (Wikidata, Mathlib)
- No Lean/Z3 verifier integration
- Semantic round-trip (factum-l latent projection) not started
- No Inspector (visual debugger)
- Identifier charset is ASCII-only (Unicode deferred to future spec version)
