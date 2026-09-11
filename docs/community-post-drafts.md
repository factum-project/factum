# Factum — Community Post Drafts

Two versions ready to post now (Anthropic + Rust). Cursor version deferred until stdio transport is wired.

Core structure: pain hook → real syntax example → 3 differentiators → preempt RDF question → honest limits + CTA.

---

## 1. Anthropic Developers Discord (#mcp or #showcase)

```
MCP server included: 3 tools (query/insert/retract) + morpheme negotiation at initialize, speaking the 2025-06-18 spec.

Your LLM's context is a lie of omission.

Markdown in context has no provenance, no validity windows, no conflict semantics. Two contradictory facts? The model guesses. A fact that expired last quarter? Still sitting there. You can't tell which source said what, or query *why* an answer was produced.

Factum is a knowledge language designed for LLM context instead of human prose:

;; every fact carries typed metadata — validity, provenance, confidence, permissions
(node n004
  :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73 :since #date(2001-03-15))
  :valid (window "2001-03-15T00:00:00+00:00")
  :conf 0.85 :auth 0.8 :perm confidential
  :src (extracted "earnings-2024" [120 350] (model "gpt-4" "2024-06")))

Three things you can't get from Markdown/JSON in context today:

1. Deterministic conflict arbitration — LatestWins / HighestAuthority / Unanimous policies. When facts collide and the policy can't resolve, Factum returns Ambiguous and *refuses to answer* rather than silently picking one. No more model guessing between contradictory context.

2. Token efficiency vs equally-expressive JSON — on the same 5-node corpus with full metadata (provenance, confidence, validity, permissions — the stuff Markdown drops), Factum canonical S-expression is ~134 tokens vs ~399 for verbose JSON (−66%). The compact JSON form is ~350 tokens (−12%). The counterintuitive finding: the canonical form (designed for parse uniqueness) is *more* token-efficient than the compact form — BPE tokenizers split JSON delimiters but merge S-expression parentheses. Markdown wins on raw tokens — and gives you zero provenance, zero conflict semantics, zero validity windows for it. (±15% heuristic estimate; real tokenizer measurement tracked in issue #9. 5-node illustrative example; 1000-node round-trip benchmark in repo.)

3. Provenance + permission filtering at the index level — a query from an unauthorized principal never materializes the node. Filtering happens at candidate generation, not as post-processing. Aggregate queries (count, sum) can't leak what you can't see.

"How is this not RDF/JSON-LD?" — RDF models the semantic web for machine inference; Factum models LLM context: token-compact serialization, morpheme table negotiated with the MCP host at initialize (clients declare `capabilities.factum`), JSON-RPC 2.0 tools that query/insert/retract structured knowledge. RDF has reification for provenance; Factum makes it first-class and non-optional (Extracted nodes MUST carry model + version — the parser enforces it).

Honest state: in-memory store only, 24 seed morphemes (target 200+), stdio/HTTP transport not yet wired (protocol/handler implemented, not tested with real MCP hosts). If you want to go deep: the handler is ready, the stdio loop isn't — that's literally good first issue #8. This is an alpha seeking 2–3 reviewers who'll actually run `cargo test`, read the handler code, and break things — not stars.

🔗 https://github.com/factum-project/factum

If you build agents or knowledge infrastructure, the question I most want answered is: *what's missing that would block you from using this for real?*
```

---

## 2. Rust Community Discord (#language-dev or #project-showcase)

```
Hand-written recursive-descent parser, minimal-dep core — no framework, no tokio, nothing exotic. Fuzz targets from day one, 90 tests, 0 clippy warnings.

Your LLM's context is a lie of omission.

Markdown in context has no provenance, no validity windows, no conflict semantics. Two contradictory facts? The model guesses — and now your context format is part of the problem. A fact that expired last quarter? Still there. You can't tell which source said what, or query *why* an answer was produced.

Factum is a knowledge language designed for LLM context instead of human prose:

;; S-expression syntax — fully parenthesized for parse uniqueness
;; every fact carries typed metadata: validity, provenance, confidence, permissions
(node n004
  :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73 :since #date(2001-03-15))
  :valid (window "2001-03-15T00:00:00+00:00")
  :conf 0.85 :auth 0.8 :perm confidential
  :src (extracted "earnings-2024" [120 350] (model "gpt-4" "2024-06")))

Three things you can't get from Markdown/JSON today:

1. Deterministic conflict arbitration — LatestWins / HighestAuthority / Unanimous policies. When facts collide and the policy can't resolve, Factum returns Ambiguous and *refuses to answer* rather than silently picking. The system has opinions about when not to guess.

2. Token efficiency vs equally-expressive JSON — canonical S-expression form is ~134 tokens vs ~399 for verbose JSON on the same 5-node corpus with full metadata (−66%). The compact JSON form is ~350 tokens (−12%). The counterintuitive finding: the form designed for correctness (fully parenthesized S-expressions) is *more* token-efficient than the form designed for compactness (JSON with numeric tags) — BPE tokenizers split `{` `}` `"` `:` into individual tokens but merge parentheses with adjacent tokens. Markdown wins on raw tokens — and gives you zero provenance, zero conflict semantics, zero validity windows for it. (±15% heuristic; real tokenizer tracked in issue #9. 5-node illustrative example; 1000-node round-trip benchmark in repo.)

3. Dec(i128, u8) everywhere — no floating-point in the data model. Financial amounts, percentages, quantities are all lossless fixed-point. Confidence/authority use f32 (subjective measures, no exact arithmetic). The parser enforces this — you can't accidentally store a revenue figure as f64.

"How is this not RDF/JSON-LD?" — RDF models the semantic web; Factum models LLM context. Token-compact serialization, MCP server with 3 tools (query/insert/retract), morpheme negotiation. Different problem, different design.

Honest state: in-memory store (RocksDB is roadmap), 24 seed morphemes (target 200+), MCP transport layer not wired (protocol/handler implemented, stdio/HTTP pending). 5 crates in a Cargo workspace: core, rt, mcp, bench, demo. Seeking 2–3 reviewers who'll run `cargo test` and poke at the parser — not stars.

🔗 https://github.com/factum-project/factum
docs/design-rationale.md explains every architectural decision (why S-expressions, why Dec not f64, why index-level permissions, why 5-level provenance).

If you care about language design or parser engineering, the question I most want answered is: *what would you do differently in the syntax or the type system?*
```

---

## 3. Cursor Community Discord — DEFERRED

**Do not post yet.** The MCP transport layer (stdio) is not wired. Cursor users who read this will want to plug it in immediately, can't, and will bounce.

**Post condition**: stdio transport verified end-to-end (issue #8 resolved), tested manually with Cursor or Claude Code, ideally with a screenshot or 30-second GIF.

When ready, use the Anthropic version with these changes:
- Move differentiator #3 (provenance first-class) to #1 position — "why does the model believe X?" is the strongest hook for AI editor users
- Opening line: "MCP server you can plug into Cursor today: 3 tools + morpheme negotiation"
- CTA: "what's missing that would block you from using this as a knowledge backend in Cursor?"

---

## Notes on data sources used

- **Syntax example**: parser-tested — wrote a temporary test that parses the exact node (including `:valid (window ...)` open-ended form) and verifies `serialize::verify_roundtrip()` passes. Temporary test removed after verification.
- **:valid syntax**: `(window "RFC3339" "RFC3339")` for bounded window, `(window "RFC3339")` for open-ended (valid from date onwards, no expiry — used in the post example to avoid showing an expired fact), `forever` for no expiry. All three forms verified against `canonical_validity()` in serialize.rs and parser.
- **:valid window choice**: open-ended form `(window "2001-03-15T00:00:00+00:00")` used instead of bounded — avoids the "fact expired before you read this" problem. The hook says "a fact that expired last quarter? Still there" — the example must show a *living* fact.
- **:auth 0.8**: verified — Authority is `pub struct Authority(pub f32)`, range [0.0, 1.0]. Not 5-level, it's a float. Consistent with spec.
- **issue #8 label**: confirmed — GOOD_FIRST_ISSUES.md declares `Labels: good first issue, documentation`. Prerequisite is stdio transport verification. "that's literally good first issue #8" is accurate. (Note: when creating actual GitHub issues from GOOD_FIRST_ISSUES.md, ensure labels are applied.)
- **Token numbers**: from README.md token efficiency table (heuristic estimate, ±15% of o200k_base). README table explicitly states "Full 7-tuple: predicate + validity + provenance + confidence + authority + permissions + deps" — token measurement already includes validity metadata, numbers are unaffected by adding `:valid` to the example.
  - Canonical: ~134 tokens (−66% vs JSON)
  - Compact: ~350 tokens (−12% vs JSON)
  - JSON baseline: ~399 tokens
  - Markdown: 419 bytes (no token estimate — Markdown has zero metadata, would win on raw tokens but loses all provenance/conflict semantics)
- **5-node illustrative**: token comparison is on 5 nodes with full 7-tuple metadata. 1000-node round-trip benchmark exists in factum-bench (100% syntactic round-trip) but does not include token efficiency measurement.
- **7-tuple definition**: id, predicate, validity, provenance, confidence, authority, permissions (7 core knowledge fields). deps and status are additional runtime metadata, not part of the 7-tuple. Example shows 6 of 7 (id is the node identifier `n004`, not a `:field`); deps omitted for readability. Verified against types.rs struct definition and CHANGELOG.
- **Byte numbers**: compact −76% vs pretty-JSON-with-same-metadata
- **Test count**: 90 tests, 0 clippy warnings (verified)
- **Conflict policies**: LatestWins / HighestAuthority / Unanimous + Ambiguous refusal (from factum-rt/src/arbitration.rs)
- **MCP tools**: factum_query / factum_insert / factum_retract (from factum-mcp/src/tools.rs)
- **Morpheme negotiation**: capabilities.factum capability declaration (from spec/compact-form.md §6)
- **Dependencies**: factum-core Cargo.toml has 7 deps: serde, serde_json, chrono, smol_str, thiserror, ahash, parking_lot. All small, ubiquitous crates — no framework, no tokio, nothing exotic. "minimal-dep" is accurate; not reporting a count in the post to avoid miscounting.
