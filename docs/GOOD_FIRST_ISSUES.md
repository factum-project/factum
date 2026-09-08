# Good First Issues

These issues are curated for first-time contributors. Each one is self-contained,
doesn't require deep understanding of the entire architecture, and has a clear
acceptance criteria.

If you want to work on one, comment "I'd like to work on this" and we'll assign it to you.

---

## 1. Add 10 new seed morphemes (Easy)
**Labels**: `good first issue`, `morpheme-proposal`
**Estimated effort**: 1-2 hours

Add 10 new morphemes to the seed vocabulary in `crates/factum-core/src/morphemes.rs` (function `seed_morphemes`). Current count is 24; target is 200+.

Suggested additions:
- `employs` (Relation: org, person, since, until? -> Assertion)
- `competes-with` (Relation: org, org -> Assertion)
- `product-of` (Relation: product, org -> Assertion)
- `headquartered-in` (Relation: org, location -> Assertion)
- `market-cap` (Relation: org, date, Dec -> Assertion)
- `subsidiary-count` (Relation: org, date, Int -> Assertion)
- `merged-into` (Relation: org, org, date -> Assertion)
- `legal-name` (Relation: org, Str -> Assertion)
- `stock-listed-on` (Relation: org, exchange, since -> Assertion)
- `industry-of` (Relation: org, Str -> Assertion)

**Acceptance**: Each morpheme has name, kind, signature, and doc string. `cargo test` passes.

---

## 2. Add conformance test vectors for edge cases (Easy→Medium)
**Labels**: `good first issue`, `testing`, `spec`
**Estimated effort**: 2-3 hours

**Important spec note**: For v0.1-alpha, Entity and Symbol identifiers are limited to **ASCII** (`[a-zA-Z][a-zA-Z0-9_-]*`). Unicode entity names (e.g., `@Sao-Paulo`, `@华为`) are explicitly deferred to a future spec version pending a decision on Unicode XID_Start/XID_Continue vs ASCII-only. NFC normalization is also a deferred decision. Do not add Unicode test vectors until the charset decision is made — that's a spec-level architectural decision, not a test vector.

Add new JSON test vectors to `spec/conformance/` covering:
- Very long string literals (>1000 chars)
- Multiple named args in different orders (should all be valid if after positional)
- Zero-scale decimal (`42` vs `42.0`) — verify both produce `Dec(42, 0)`
- Empty list argument `[@X []]`
- Date with different formats
- String literals containing `?` and `@` characters (disambiguation for compact form)
- Pure numeric string literal vs Dec literal

**Acceptance**: Vectors pass in the Rust conformance runner (`cargo test -p factum-core --test conformance`). No Unicode identifiers — ASCII only for v0.1.

---

## 3. Improve parser error messages (Medium)
**Labels**: `good first issue`, `parser`
**Estimated effort**: 2-3 hours

Current error messages are functional but could be more helpful. For example:
- "expected symbol 'node'" could say "expected 'node' keyword but found 'Node' (did you mean 'node'?)"
- Missing `:pred` error could suggest the syntax: "node missing :pred field — try: :pred (your-morpheme args...)"

Look at `crates/factum-core/src/parser.rs` and improve 5-10 error messages.

**Acceptance**: New error messages are more helpful. Existing tests still pass. Add new tests verifying the improved messages.

---

## 4. Add compact serialization Rust-side round-trip + edge-case conformance vectors (Medium)
**Labels**: `good first issue`, `serialization`
**Estimated effort**: 2-3 hours

> **Note**: Core compact conformance vectors already exist in `spec/conformance/compact_basic.json` (3 vectors: `?`/`@` string disambiguation, open-ended validity, round-trip identity). This issue covers the **remaining** work: Rust-side canonical equivalence tests and edge-case expansion.

**Part A — Rust-side canonical equivalence test** (in `crates/factum-core/src/serialize.rs` tests module):

Write a test that:
1. Creates a Node
2. Serializes to compact JSON
3. Deserializes the JSON back to a CompactNode
4. Verifies **canonical equivalence**: `parse(canonical(x))` and `deserialize(compact(x))` produce semantically identical Node structures (same id, predicate, validity, provenance, confidence, authority, permissions, deps)

**Part B — Edge-case conformance vectors** (in `spec/conformance/compact_edge_cases.json`):

- Morpheme fallback: head as string name when no registry (unregistered morpheme name stays as string, not u32 index)
- CompactValidity `Forever` vs open-ended Window (both should round-trip distinctly)
- Nested compound predicate in compact form
- All 5 provenance types in compact form
- Empty args list
- Multiple named args

**Acceptance**: Tests pass. Canonical equivalence holds. Edge-case vectors pass in the conformance runner.

---

## 5. Add query benchmark with 100K nodes (Medium)
**Labels**: `good first issue`, `benchmark`
**Estimated effort**: 2-3 hours

Current `factum-bench/src/query_perf.rs` tests with 10K nodes. Add a variant with 100K nodes and measure:
- Lookup by entity (single match)
- Lookup by predicate head (many matches)
- Pattern match with variable binding
- Query with permission filtering

**Acceptance**: Benchmark runs without timeout. Results are printed. No assertion failures.

---

## 6. Add Wikidata P31 mapping table (Medium)
**Labels**: `good first issue`, `corpus`
**Estimated effort**: 3-4 hours

Create a mapping table from Wikidata properties to Factum morphemes in `tools/factum-convert/src/wikidata_mapping.toml`:
- P31 (instance of) → `instance-of`
- P17 (country) → `located-in` (with location semantics)
- P159 (headquarters location) → `headquartered-in`
- P361 (part of) → `subsidiary-of`
- P112 (founded by) → `founded-by` (new morpheme needed)
- P169 (CEO) → `ceo-of`
- P355 (subsidiaries) → `subsidiary-of`

Each mapping should include: Wikidata property ID, Factum morpheme name, argument mapping (which Wikidata statement fields map to which Factum Term positions).

**Acceptance**: TOML file is valid. Each mapping has at least one example. Documentation explains the mapping rationale.

---

## 7. Add span-based source location tracking to parser errors (Harder)
**Labels**: `good first issue`, `parser`
**Estimated effort**: 4-6 hours

Currently parser errors report line/col of the error token. Improve this to also show:
- A caret pointing at the error position in the source
- 1-2 lines of context around the error

Example desired output:
```
Parse error at line 3 col 15: expected ')' but found ':conf'
  2 |   :pred (instance-of @X organization)
  3 |   :conf 0.85 :perm public)
              ^
```

**Acceptance**: Error messages include source context. Existing tests pass. New tests verify the context display.

---

## 8. Write a "Getting Started" guide for MCP integration (Easy→Medium)
**Labels**: `good first issue`, `documentation`
**Estimated effort**: 2-4 hours

**Prerequisite**: The stdio transport layer needs to be verified first. Currently the MCP handler is only tested via unit tests — it has never read from real stdin or written to real stdout. Before writing this guide, verify that stdio mode works end-to-end.

Write `docs/getting-started-mcp.md` explaining:
1. How to start the Factum MCP server (stdio mode) — include a minimal `main.rs` that reads JSON-RPC from stdin and writes responses to stdout
2. How to configure it in Claude Code / Cursor
3. Example queries using the `factum_query` tool
4. How to insert knowledge and read resources

**Acceptance**: Guide is clear enough for someone unfamiliar with Factum to follow. The stdio transport is verified to work end-to-end (not just unit tests). Code examples are tested manually with a real MCP client or `echo` + `jq` piped to the server binary.

---

## 9. Replace heuristic token estimator with real tokenizer (Medium)
**Labels**: `good first issue`, `benchmark`, `llm-native`
**Estimated effort**: 2-3 hours

The current token efficiency test in `crates/factum-bench/src/token_efficiency.rs` uses a heuristic estimator (`estimate_tokens()`) that approximates o200k_base behavior within ±15%. This is a placeholder — the real metric that matters for the "LLM-native language" narrative is actual token counts.

Replace the heuristic with real tokenizer measurement:
1. Add `tiktoken-rs` as a dev-dependency to `factum-bench`
2. Use `tiktoken_rs::o200k_base()` (GPT-4o) and `cl100k_base()` (GPT-4) tokenizers
3. Measure compact form vs canonical vs verbose JSON in real tokens
4. Update README token efficiency table with real numbers
5. If real token savings differ significantly from heuristic estimates, update the narrative accordingly

**Acceptance**: Real tokenizer runs in CI without timeout. README shows actual token counts. Heuristic estimator is removed or clearly marked as fallback.
