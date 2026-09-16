# Factum Roadmap

This roadmap tracks what's planned, in priority order. Items before the M2 line are **promotion blockers** — Factum will not be publicly promoted until they are done.

## Legend
- ✅ Done
- 🚧 In progress
- 📋 Planned
- 🔬 Research (uncertain timeline)

---

## M0: Core Implementation ✅ (2026-09-08)

- ✅ factum-core: data model, lexer, parser, serialization
- ✅ factum-rt: in-memory store, query, arbitration, permissions, verifiers
- ✅ factum-mcp: protocol layer + request handler (JSON-RPC 2.0, MCP 2025-06-18)
- ✅ factum-mcp: stdio transport (verified end-to-end with real stdin/stdout)
- 📋 factum-mcp: Streamable HTTP transport (requires web framework dependency)
- ✅ factum-bench: round-trip, token efficiency, query perf
- ✅ All tests passing (see CI badge in README)
- ✅ Parser depth limit + lexer token limit (DoS protection)
- ✅ cargo-fuzz targets (parser, serialize, lexer)

## M1: Open-Source Alpha 🚧 (target: 2026-09-15)

- ✅ README with precise capability/status alignment
- ✅ SECURITY.md, CONTRIBUTING.md, CHANGELOG.md, CODE_OF_CONDUCT.md
- ✅ CI: test + clippy + fuzz + gitleaks
- ✅ DCO sign-off policy
- ✅ Trademark notice
- ✅ LICENSE (MIT, Factum Project Contributors)
- ✅ Conformance test vectors (JSON, language-agnostic) — `spec/conformance/`
- ✅ Compact form wire format spec (`spec/compact-form.md`, v0.1-draft)
- ✅ "Relationship to other formats" section in README
- ✅ "Identifier Character Set" policy (ASCII-only for v0.1)
- ✅ Design rationale document (`docs/design-rationale.md`)
- ✅ Placeholder emails replaced with `@factum.dev` + GitHub Security Advisory fallback
- ✅ GitHub org URL unified (`factum-project/factum`)
- ✅ Version: `0.1.3`
- ✅ Benchmark numbers unified (pretty-JSON baseline)
- ✅ Token efficiency table (real o200k_base measurement via tiktoken-rs; issue #9 resolved)
- ✅ LLM authoring guide (`docs/authoring-for-llms.md`)
- ✅ Good first issues (9 pre-labeled, `docs/GOOD_FIRST_ISSUES.md`)
- ✅ Issue/PR templates (bug report, feature proposal, morpheme proposal, PR template)
- ✅ Clippy: 0 warnings (`--all-targets`)

**M1 exit criteria**: fuzzing CI stable for ≥1 week with no uncrashed bugs, all infrastructure files in place, 3+ external reviewers have looked at the code.

## M2: Promotion-Ready 📋 (target: 2026 Q4)

These are **hard blockers** for any public promotion (Show HN, blog posts, conference talks):

### Agent Memory Core
- ✅ `StorageBackend` trait abstraction (InMemory + RocksDB backends, feature-gated)
- ✅ RocksDB backend with 5 column families (nodes + 4 secondary indices)
- ✅ `batch_write` for atomic multi-node writes (RocksDB WriteBatch)
- ✅ Persistence verified: reopen database retains all data (12 tests passing)
- ✅ Cascade retraction via reverse dependency graph (`deps_rev`)
- ✅ `by_validity` BTreeMap index used for temporal queries (range query, not full scan)
- ✅ Conflict arbitration with `Ambiguous` refusal (LatestWins, HighestAuthority, Unanimous, Custom)
- 📋 MVCC optimistic concurrency with merge/abort policy

### MCP Integration
- ✅ stdio transport verified end-to-end (real stdin/stdout, not just unit tests)
- ✅ `preferred_form` negotiation: canonical S-expression serving for LLM clients
- ✅ Store ↔ SubscriptionManager integration: insert/retract notifications wired
- ✅ Store ↔ VerifierRegistry integration: optional pre-insert verification (opt-in)
- ✅ `tools/listChanged` capability declared (MCP clients re-query tools on reconnect)
- ✅ 8 MCP tools: query, lookup, insert, insert_batch, upsert, assert, search, retract
- ✅ 42 handler unit tests covering all tools (normal + error + edge cases)
- ✅ StoreError → MCP error code mapping (invalid_params vs internal)
- 📋 Real Claude Code integration test (not self-tested handler)
- 📋 Real Cursor integration test
- 📋 Streamable HTTP transport (requires web framework: axum or hyper)
- 📋 Inspector prototype (TS/WASM visual debugger)

### Vocabulary
- ✅ Morpheme table expanded from 24 to **200+** seeds across 9 kinds (Entity, Relation, Quantifier, Modal, Temporal, Status, Action, Attribute, Classification)
- 📋 `morphemes.toml` format + `build.rs` codegen

### Benchmarks
- ✅ **P0**: Replace heuristic token estimator with real tokenizer (issue #9 ✅) — **confirmed**: canonical −62% tokens, compact −53% tokens
- ✅ **Form-positioning decision**: `capabilities.factum.preferred_form` negotiation implemented. When `"canonical"`, query results return as S-expression text (−62% tokens for LLM context). When absent or `"compact"`, defaults to compact JSON.
- 📋 Grounded QA benchmark (multi-hop fact QA with verifiable citations)

### Fuzzing
- 📋 Fuzzing CI stable for **≥4 weeks** with no uncrashed crashes
- 📋 OSS-Fuzz integration (optional)

### Conformance
- 📋 Test vectors separated from Rust implementation
- 📋 Python SDK passes conformance suite
- 📋 TypeScript SDK passes conformance suite

### Documentation
- ✅ Confidence & authority calibration guide (`docs/confidence-calibration.md`)
- 📋 Compact form vs Markdown/JSON efficiency data (published)
- 📋 Factum-loses dimensions explicitly shown (esp. vs Markdown)

### Corpus & Verification (lower priority for agent memory use case)
- 📋 Wikidata converter PoC (≥1M nodes)
- 📋 Dead-letter queue + manual review workflow
- 📋 Mathlib converter (Lean 4 AST → Derived nodes)
- 📋 Z3 solver verifier (`SolverVerifier` via `z3.rs`)
- 📋 Lean process-pool verifier (`LeanVerifier` with 10s timeout)

## M3+: Research 📋

### Confidence Feedback Loop 📋
- 📋 `compute_reliability_table()` — aggregate `(provenance_type, model, principal)` stats: total/active/retracted → observed accuracy. Uses existing `Extracted` mandatory `ModelRef` and `Asserted` `Principal` fields. The `principal` dimension enables per-agent reliability tracking in multi-agent scenarios (same model, different agent instances may have different accuracy).
- 📋 `default_confidence_for_provenance()` auto-switch — when N≥10 data points exist for a `(provenance, model, principal)` triple, use observed accuracy instead of policy constants. Cold start falls back to `calibration.rs` constants. When principal is unknown or "system", falls back to `(provenance, model)` pair.
- 📋 Retract reason classification — add optional `reason` parameter to retract: `"error"` / `"outdated"` / `"superseded"` / `"cleanup"`. Only `error` and `superseded` count against model reliability score.
- 📋 `corroboration_count(predicate_hash)` — count independent `(principal, model)` pairs asserting the same canonical predicate. ≥2 pairs allows confidence above band ceiling. Content-addressed IDs naturally prevent same-pair double-counting.
- 📋 Band clipping enforcement in `factum_assert` — reject or clamp `:conf` values exceeding provenance band, with corroboration check for exceptions. Currently emits warnings only (M2 behavior).

### factum-l: Latent Space Projection 🔬
- 🔬 Encoder E (Transformer) → continuous thought vector z
- 🔬 Decoder D(z) → Factum-F sequence
- 🔬 Semantic round-trip target: >=0.95 (v0.1), >=0.99 (acceptance threshold)
- 🔬 SemEquiv structured comparison (not vector cosine)
- 🔬 Training pipeline (Qwen2.5-7B-Instruct base + LoRA)
- 🔬 Weak decoder baseline first — measure before optimizing

### Governance
- 📋 Morpheme proposal → review → adoption process
- 📋 Spec versioning and backwards compatibility policy
- 📋 Potential foundation donation (CLA may be required at this stage)

---

## Not on the Roadmap (Explicitly)

These are deliberately excluded:

- **No agent framework** — Factum is a memory layer via MCP, not a competitor to LangGraph/CrewAI. Framework-agnostic by design.
- **No GPU inference in core crates** — factum-l is a separate concern
- **No web frontend** — Inspector is TS/WASM, but it's a debugger, not a product
- **No cloud hosting** — Factum is a library/protocol, not a SaaS
- **No paid tier** — MIT licensed, period
- **No embedding-based semantic search** — consider using Mem0 alongside Factum for that capability
- **No memory consolidation/summarization** — consider using Letta alongside Factum for that capability

---

## Specifications

- [Compact Form Wire Format](spec/compact-form.md) (compact-form v0.1-draft) — JSON encoding for MCP transport
- [Conformance Test Vectors](spec/conformance/) — language-agnostic parser test suite
