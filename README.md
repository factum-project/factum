# Factum — A Native Knowledge Language for LLMs

> **Status: v0.1.0 — Working draft, seeking early collaborators.**
> Not a release. Not production-ready. Architectural decisions are still open to change.

![CI](https://github.com/factum-project/factum/actions/workflows/ci.yml/badge.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)
![MCP Registry](https://img.shields.io/badge/MCP-Registry-registered)

![Factum MCP Demo](docs/demo/factum-mcp-demo.png)

> **[Interactive docs](https://factum-project.github.io/factum/)** — animated syntax parsing, 7-tuple explorer, query pipeline, token efficiency chart, and MCP architecture diagram.

Factum is a structured knowledge representation language designed as a **native format for LLMs** — not a human-facing database format. LLMs read it as context, write it as output, and (in the roadmap's endgame) think in its latent-space projection. Every design decision serves LLM-native usage: full parenthesization for parse-safe LLM generation, error class taxonomy for LLM self-correction, compact form for context economy, `Dec(i128, u8)` to catch LLM numeric hallucinations, and mandatory model references on extracted knowledge for LLM self-auditing.

## Three-Layer Vision

| Layer | What It Means | Status |
|-------|--------------|--------|
| **LLM Read** | LLM receives Factum-F as context payload via MCP — lower token overhead than verbose JSON | ✅ Architecture ready, token efficiency measured (real o200k_base: canonical −62%, compact −53% vs JSON) |
| **LLM Write** | LLM generates Factum-F nodes directly — parse uniqueness guarantees one valid interpretation, error classes enable self-correction | ✅ Architecture ready, see [authoring guide](docs/authoring-for-llms.md) (draft) |
| **LLM Think** | factum-l: encode Factum-F into continuous thought vector, LLM reasons in latent space, decode back for audit | 🔬 M3 research item — not started, not blocking layers 1-2 |

Enterprise provenance, RAG enhancement, and MCP knowledge bases are **where this language first delivers value** — but they are realization paths, not the definition. The definition is: a language LLMs can use as their native knowledge medium.

## What's Implemented vs. What's Not

| Module | Status | Notes |
|--------|--------|-------|
| factum-core (types, lexer, parser, serialize) | ✅ Implemented | 100% syntactic round-trip |
| factum-rt (store, query, arbitration, permissions, verifiers) | ✅ Implemented | In-memory store (default) + **RocksDB persistence backend** (`--features rocksdb`, feature-gated); 5 column families, WriteBatch atomic writes, crash-durable |
| factum-mcp (JSON-RPC bridge) | ✅ Protocol + handler | Protocol layer + request handler implemented; **stdio transport verified** end-to-end (real stdin/stdout, MCP notification handling, resource discovery); HTTP transport not yet implemented; see [Getting Started](docs/getting-started-mcp.md) for Claude Code / Cursor setup |
| factum-bench (benchmarks) | ✅ Implemented | Syntax round-trip + token efficiency + query perf |
| factum-l (latent space projection) | ❌ Not started | Planned, research-grade — see ROADMAP.md |
| Wikidata/Mathlib corpus converters | ❌ Not started | M2 milestone |
| Lean/Z3 verifiers | ❌ Not started | Only Schema + DecimalRange verifiers implemented |
| Inspector (visual debugger) | ❌ Not started | |

## Key Features (Implemented)

- **Node 7-tuple**: Every knowledge node carries id, predicate, validity, provenance, confidence, authority, and permissions
- **Lossless numerics**: All numbers use `Dec(i128, u8)` — zero floating-point error
- **5-level provenance**: Verbatim / Summary / Extracted / Derived / Asserted — full audit chain
- **Index-level permissions**: No post-query filtering — prevents aggregate leakage
- **Conflict arbitration**: LatestWins / HighestAuthority / Unanimous — refuses to guess when ambiguous
- **Cascade retraction**: Derived nodes auto-invalidate when upstream sources are retracted
- **MCP bridge**: JSON-RPC 2.0 tools/resources with morpheme table negotiation for token efficiency; **stdio transport verified** with real MCP clients (Claude Code, Cursor)

## Round-Trip Fidelity — What Exactly Is 100%?

There are two distinct definitions of "round-trip fidelity":

1. **Syntactic round-trip** ✅ — `parse(serialize(parse(x))) == parse(x)`. This means: if you parse Factum-F source, serialize it back to canonical form, and parse again, you get the same AST. **This is 100% and verified by the test suite + fuzzing.** This is the trust foundation of the system.

2. **Semantic round-trip** ❌ Not yet measured — This is about the latent-space projection (factum-l): encode Factum-F into a continuous thought vector `z`, run LLM inference in `z` space, decode back to Factum-F', and check that `F'` is semantically equivalent to `F` (via `SemEquiv >= 0.99`). **This requires factum-l, which is not implemented.** The v0.1 target is >=0.95; 0.99 is the acceptance threshold.

If you see "100% round-trip" anywhere in this repo, it refers to **syntactic** round-trip only.

## Morpheme Vocabulary — Current State

- **Seed morphemes**: 24 (covering common entity types, relations, quantifiers, modals, and temporal operators)
- **Design target**: 200–500 (to be loaded from `morphemes.toml` via `build.rs`)
- **Gap**: The current 24 seed morphemes are sufficient for testing the architecture but **not sufficient for production use**. Expanding the vocabulary is a pre-M2 requirement.

## Identifier Character Set

For v0.1-alpha, Entity names (`@Foo`), Symbols, and Node IDs are limited to **ASCII**: `[a-zA-Z][a-zA-Z0-9_-]*`. Unicode identifier support and NFC normalization are deferred to a future spec version pending a decision on Unicode XID_Start/XID_Continue vs ASCII-only. This is a spec-level architectural decision that will be made before M2.

## Project Structure

```
factum/
├── crates/
│   ├── factum-core/     # Data model, lexer, parser, serialization
│   ├── factum-rt/       # Runtime: store (InMemory + RocksDB), query, arbitration, permissions, verifiers
│   ├── factum-mcp/      # MCP bridge: JSON-RPC tools/resources, stdio transport
│   ├── factum-bench/    # Benchmarks: round-trip, token efficiency, query perf
│   └── factum-demo/     # End-to-end demonstration
├── fuzz/              # cargo-fuzz targets (parser, serialize round-trip, lexer)
├── spec/              # Conformance test vectors (JSON, language-agnostic)
├── docs/site/         # Interactive visualization (GitHub Pages)
├── .github/workflows/ # CI: test + fuzz + gitleaks + Pages deploy
├── Cargo.toml         # Workspace root
├── ROADMAP.md         # What's planned and in what order
├── SECURITY.md        # Vulnerability disclosure
├── CONTRIBUTING.md    # How to contribute
├── CHANGELOG.md       # Version history
├── docs/design-rationale.md     # Why each architectural decision was made
├── docs/authoring-for-llms.md   # LLM guide for generating Factum-F
└── docs/getting-started-mcp.md  # MCP setup guide (Claude Code / Cursor)
```

## Quick Start

```bash
# Build
cargo build

# Run tests
cargo test

# Run demo
cargo run -p factum-demo

# Build with RocksDB persistence backend
cargo build --features rocksdb
cargo test --features rocksdb

# Fuzz (requires nightly)
cargo +nightly fuzz run fuzz_parser -- -max_total_time=600
```

### Use as an MCP server (Claude Code / Cursor)

```bash
# Build the MCP server binary
cargo build --release -p factum-mcp --bin factum-mcp-server

# Register with Claude Code
claude mcp add --transport stdio --scope local factum -- "$(pwd)/target/release/factum-mcp-server"

# Verify connection
claude mcp get factum
```

See [Getting Started with Factum MCP](docs/getting-started-mcp.md) for the full guide.

## Factum-F Syntax Example

```scheme
; Acme Corp knowledge graph
(node n001
  :pred (instance-of @ACME-CORP organization)
  :conf 0.99 :auth 0.95 :perm public
  :src (asserted "wikidata"))

(node n004
  :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73 :since #date(2001-03-15))
  :conf 0.85 :auth 0.8 :perm confidential
  :src (extracted "doc002" [100 200] (model "gpt-4" "2024-06")))

(node n006
  :pred (subsidiary-of @ACME-SUB @ACME-CORP :since #date(2001-03-15))
  :src (derived n001 "rule-subsidiary-merge")
  :deps [n001])
```

## Token Efficiency — The LLM-Native Metric

> **TL;DR: Factum compact form saves ~68% bytes and ~54% tokens vs verbose JSON. Canonical form saves ~62% tokens — the form designed for correctness is also the most token-efficient. All numbers measured with real o200k_base (GPT-4o) tokenizer via tiktoken-rs.**

Bytes matter for storage; **tokens matter for LLMs**. A format that saves bytes but not tokens doesn't help an LLM's context window. Here's the full picture:

### Byte Efficiency

All byte percentages use **pretty JSON with the same 7-tuple metadata** as the baseline.

| Format | Bytes (5 nodes) | vs pretty JSON | What it includes |
|--------|-----------------|----------------|------------------|
| Factum compact (JSON) | 643 | **−68%** | Full 7-tuple: morpheme indices + numeric tags |
| Factum canonical | 650 | −67% | Full 7-tuple: predicate + validity + provenance + confidence + authority + permissions + deps |
| Markdown | 420 | −79% | Only the assertion text — no provenance, no confidence, no permissions |
| JSON (pretty) | 1994 | baseline | Same 7-tuple metadata in verbose JSON encoding |

### Token Efficiency (measured with real o200k_base tokenizer — GPT-4o)

| Format | Real tokens (5 nodes) | vs verbose JSON | Notes |
|--------|-----------------------|-----------------|-------|
| Factum compact (JSON) | 290 | **−53%** | JSON keys replaced by numeric tags; significantly fewer tokens than verbose JSON |
| Factum canonical | 238 | **−62%** | S-expression is the most token-efficient form — BPE merges parens with adjacent tokens |
| Markdown | 181 | −71% | No metadata at all — unfair comparison (no provenance, no confidence) |
| JSON (pretty) | 623 | baseline | Verbose keys (`"provenance"`, `"confidence"`) each cost multiple tokens |

> **✅ These are real tokenizer measurements** (o200k_base / GPT-4o via `tiktoken-rs`). Run `cargo test -p factum-bench test_token_efficiency_real_tokenizer -- --nocapture` to reproduce.

**Key finding — canonical beats compact on tokens**: The canonical S-expression form (238 tokens) is more token-efficient than the compact JSON form (290 tokens). BPE tokenizers split JSON delimiters (`{`, `}`, `"`, `:`) into individual tokens, while S-expression parentheses and whitespace are frequently merged with adjacent tokens. **The form designed for correctness is also the most token-efficient form for LLM context windows.**

**Form-positioning implication**: This confirms the initial finding. The implemented `capabilities.factum.preferred_form` negotiation (see `spec/compact-form.md` §8) serves canonical to LLM clients and repositions compact as a storage/service-to-service format.

**The fair comparison is Factum compact vs pretty-JSON-with-same-metadata** — compact saves 68% bytes and 53% tokens.

## Relationship to Other Formats

Factum is not a replacement for any existing format. It occupies a specific niche: structured knowledge representation designed for LLM read/write/reason with built-in provenance and verifiability.

| Format | What It Is | How Factum Differs |
|--------|-----------|-------------------|
| **RDF / JSON-LD** | W3C semantic web standard: triples (subject, predicate, object) | Factum nodes are 7-tuples (not triples), carrying provenance, confidence, validity, authority, and permissions per-node. RDF has reification for provenance; Factum makes it first-class. |
| **CUE** | Configuration language with validation and codegen | CUE validates configuration; Factum validates *knowledge claims* with temporal validity, conflict arbitration, and cascade retraction. Different domain. |
| **Datalog** | Logic programming language for deductive queries | Factum supports pattern-matching queries (Datalog-like), but adds temporal validity, confidence-weighted arbitration, and provenance tracking. Factum is not Turing-complete by design. |
| **Markdown** | Human-readable text format | Markdown is for humans. Factum-F is for LLMs — it trades human readability for machine verifiability and lossless round-trip. |
| **JSON** | Generic data interchange format | JSON has no schema, no provenance, no temporal validity. Factum compact form uses JSON as a transport encoding but adds structure, types, and audit chain. |

**When to use what**:
- Use **RDF/JSON-LD** if you need SPARQL endpoints and W3C ecosystem compatibility
- Use **CUE** if you're validating application configuration
- Use **Datalog** if you need deductive inference over a rule base
- Use **Factum** if you need LLMs to natively read, write, and (eventually) think in a structured knowledge format with verifiable provenance, confidence, and temporal validity

## Test Results

Run `cargo test` to see the current count. Tests cover: types, lexer, parser, serialization, morphemes, depth-limit/DoS protection, conformance vectors, store, query, arbitration, permission, verifier, subscription, MCP protocol/tools/handler, and benchmarks.

CI badge at the top of this README reflects the latest build status.

## Security

- Parser has a **depth limit** (128 levels) to prevent stack overflow DoS on adversarial input
- Lexer has a **token count limit** (1M tokens) to prevent OOM
- **cargo-fuzz** targets for parser, serialize round-trip, and lexer
- **gitleaks** runs in CI to scan for leaked secrets

See [SECURITY.md](SECURITY.md) for vulnerability disclosure.

## License

MIT

## Trademark

"Factum" and the Factum-F language specification are project names. This MIT license covers code only; the specification may be governed by a separate process in the future.

## Contributing

Contributions require **DCO sign-off** (`git commit -s`). See [CONTRIBUTING.md](CONTRIBUTING.md).

---

- MCP Registry name: `mcp-name: io.github.factum-project/factum`
