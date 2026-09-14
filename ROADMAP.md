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
- ✅ Version: `0.1.0`
- ✅ Benchmark numbers unified (pretty-JSON baseline)
- ✅ Token efficiency table (real o200k_base measurement via tiktoken-rs; issue #9 resolved)
- ✅ LLM authoring guide (`docs/authoring-for-llms.md`)
- ✅ Good first issues (9 pre-labeled, `docs/GOOD_FIRST_ISSUES.md`)
- ✅ Issue/PR templates (bug report, feature proposal, morpheme proposal, PR template)
- ✅ Clippy: 0 warnings (`--all-targets`)

**M1 exit criteria**: fuzzing CI stable for ≥1 week with no uncrashed bugs, all infrastructure files in place, 3+ external reviewers have looked at the code.

## M2: Promotion-Ready 📋 (target: 2026 Q4)

These are **hard blockers** for any public promotion (Show HN, blog posts, conference talks):

### Storage
- ✅ `StorageBackend` trait abstraction (InMemory + RocksDB backends, feature-gated)
- ✅ RocksDB backend with 5 column families (nodes + 4 secondary indices)
- ✅ `batch_write` for atomic multi-node writes (RocksDB WriteBatch)
- ✅ Persistence verified: reopen database retains all data (12 tests passing)
- 📋 MVCC optimistic concurrency with merge/abort policy

### MCP Integration
- ✅ stdio transport verified end-to-end (real stdin/stdout, not just unit tests)
- ✅ `preferred_form` negotiation: canonical S-expression serving for LLM clients
- ✅ Store ↔ SubscriptionManager integration: insert/retract notifications wired
- ✅ Store ↔ VerifierRegistry integration: optional pre-insert verification (opt-in)
- 📋 Streamable HTTP transport (requires web framework: axum or hyper)
- 📋 Real Claude Code integration test (not self-tested handler)
- 📋 Real Cursor integration test
- 📋 Inspector prototype (TS/WASM visual debugger)

### Corpus
- 📋 Wikidata converter PoC (≥1M nodes)
- 📋 Dead-letter queue + manual review workflow
- 📋 Mathlib converter (Lean 4 AST → Derived nodes)

### Vocabulary
- 📋 Morpheme table expanded from 24 to **200+** seeds
- 📋 `morphemes.toml` format + `build.rs` codegen

### Verification
- 📋 Z3 solver verifier (`SolverVerifier` via `z3.rs`)
- 📋 Lean process-pool verifier (`LeanVerifier` with 10s timeout)

### Fuzzing
- 📋 Fuzzing CI stable for **≥4 weeks** with no uncrashed crashes
- 📋 OSS-Fuzz integration (optional)

### Benchmarks
- ✅ **P0**: Replace heuristic token estimator with real tokenizer (issue #9 ✅) — **confirmed**: canonical −62% tokens, compact −53% tokens
- ✅ **Form-positioning decision**: `capabilities.factum.preferred_form` negotiation implemented. When `"canonical"`, query results return as S-expression text (−62% tokens for LLM context). When absent or `"compact"`, defaults to compact JSON.
- 📋 Compact form vs Markdown/JSON efficiency data (published)
- 📋 Factum-loses dimensions explicitly shown (esp. vs Markdown — Markdown likely wins on tokens due to zero metadata)
- 📋 Grounded QA benchmark (multi-hop fact QA with verifiable citations)

### Conformance
- 📋 Test vectors separated from Rust implementation
- 📋 Python SDK passes conformance suite
- 📋 TypeScript SDK passes conformance suite

### Documentation
- ✅ Confidence & authority calibration guide (`docs/confidence-calibration.md`)
- 📋 Compact form vs Markdown/JSON efficiency data (published)
- 📋 Factum-loses dimensions explicitly shown (esp. vs Markdown)

## M3+: Research 📋

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

- **No GPU inference in core crates** — factum-l is a separate concern
- **No web frontend** — Inspector is TS/WASM, but it's a debugger, not a product
- **No cloud hosting** — Factum is a library/protocol, not a SaaS
- **No paid tier** — MIT licensed, period

---

## Specifications

- [Compact Form Wire Format](spec/compact-form.md) (compact-form v0.1-draft) — JSON encoding for MCP transport
- [Conformance Test Vectors](spec/conformance/) — language-agnostic parser test suite
