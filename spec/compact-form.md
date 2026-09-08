# Factum Compact Form Wire Format

**Version:** compact-form v0.1-draft
**Status:** Draft — implemented in `factum-core::serialize::compact`, open for review.
**Last updated:** 2026-09-08

> This is a protocol specification, not a design document. Once published,
> breaking changes require a version bump. The canonical S-expression form
> is defined separately in the parser grammar.

## 1. Overview

The compact form is a JSON encoding of Factum nodes optimized for MCP transport.
It replaces verbose S-expression field names with numeric keys and resolves
morpheme names to u32 indices when a `MorphemeRegistry` is available.

**Size comparison** (5 nodes with full 7-tuple metadata):

| Format | Bytes | Reduction vs pretty JSON |
|--------|-------|--------------------------|
| Canonical S-expression | 649 | −55% |
| Compact JSON | ~350 | −76% |
| Pretty JSON (same metadata) | 1448 | baseline |
| Markdown (no metadata) | 419 | −71% |

All percentage reductions in this document use **pretty-JSON-with-same-metadata**
as the baseline, because that is the only fair comparison (same data content).

## 2. Node Encoding

A compact node is a JSON object with numeric keys:

```json
{
  "0": "n001",                  // id (string, required)
  "1": 42,                      // head: u32 morpheme index OR string name (required)
  "2": ["@ACME-CORP", "0.73"],   // args: array of CompactTerm (required, may be empty)
  "3": [["period", "#date(2024-01-01)"]],  // named: array of [key, CompactTerm] pairs (omitted if empty)
  "4": "forever",               // valid: CompactValidity (omitted → Forever)
  "5": {"t":"extracted","v":{"d":"doc001","s":[0,100],"m":["gpt-4","2024-06"]}},  // src: CompactProvenance (required)
  "6": 0.85,                    // conf: f32 (required)
  "7": 0.9,                     // auth: f32 (required)
  "8": 4,                       // perm: u32 bitmask (omitted → 1 = public)
  "9": ["n001", "n002"]         // deps: array of node id strings (omitted if empty)
}
```

**Key map:**

| Key | Field | Type | Required | Default |
|-----|-------|------|----------|---------|
| 0 | id | string | yes | — |
| 1 | head | u32 (morpheme index) or string (name) | yes | — |
| 2 | args | array of CompactTerm | yes | `[]` |
| 3 | named | array of `[string, CompactTerm]` pairs | no | `[]` (omitted if empty) |
| 4 | valid | CompactValidity | no | `Forever` (omitted) |
| 5 | src | CompactProvenance | yes | — |
| 6 | conf | f32 | yes | — |
| 7 | auth | f32 | yes | — |
| 8 | perm | u32 bitmask | no | `1` (public) |
| 9 | deps | array of string | no | `[]` (omitted if empty) |

### Why conf/auth use f32 while args use Dec

`Confidence` and `Authority` are **subjective measures** — they represent human
or model judgment, not exact quantities. Using `f32` is appropriate because
they do not participate in exact arithmetic (you never sum confidences or
compute authority differentials). `Dec(i128, u8)` is reserved for values where
precision matters: financial amounts, percentages, quantities. This is a
deliberate design decision, not an oversight.

## 3. Term Encoding (CompactTerm)

CompactTerm is an untagged JSON union. The decoder determines the variant by
inspection of the JSON value:

| JSON type | Variant | Example |
|-----------|---------|---------|
| string starting with `?` | Var | `"?x"` |
| string starting with `@` | Entity | `"@ACME-CORP"` |
| string (any other) | Literal | `"0.73"`, `"\"hello\""`, `"#date(2024-01-01)"` |
| array | List | `["@A", "@B"]` |
| object `{h, a, n?}` | Compound | `{"h":"revenue","a":["@X","100"]}` |

### Literal Disambiguation Rule

Because CompactTerm uses an untagged union, a string value could be a Var
(starts with `?`), an Entity (starts with `@`), or a Literal (everything else).
When a string literal's *content* begins with `?` or `@`, the canonical
encoding wraps it in embedded quotes to disambiguate:

- String literal `"?foo"` is encoded as `"\"?foo\""` (with embedded quotes)
- String literal `"@bar"` is encoded as `"\"@bar\""` (with embedded quotes)
- Var `?foo` is encoded as `"?foo"` (no embedded quotes)
- Entity `@bar` is encoded as `"@bar"` (no embedded quotes)

**Decoder rule**: if a string starts with `"` (embedded quote), it is a Literal
containing the unquoted string. If it starts with `?`, it is a Var. If it starts
with `@`, it is an Entity. Otherwise, it is a Literal in canonical encoding
(number, date, duration, boolean, URI).

**Numeric edge case**: A string literal containing only digits (e.g., `"42"`)
is a Literal of type `Dec(42, 0)`. A JSON number `42` (without quotes) is
invalid in CompactTerm position — all terms are JSON strings or structured
types, never bare JSON numbers. This prevents f64 precision loss during
JSON transport.

## 4. Validity Encoding (CompactValidity)

CompactValidity has two forms:

| Form | JSON | Meaning |
|------|------|---------|
| Forever | `"forever"` (string) | Node is always valid |
| Window | `{"f":"2024-01-01T00:00:00+00:00","u":"2024-12-31T00:00:00+00:00"}` | Valid from `f` to `u` (RFC3339 timestamps) |

When the `"u"` field is **omitted** in a Window object, the validity is
**open-ended** (valid from `f` with no expiry). This is distinct from `Forever`,
which has no start date either.

When the entire `"4"` (valid) key is omitted from the node object, the default
is `Forever`.

## 5. Provenance Encoding (CompactProvenance)

Tagged union with `"t"` (type) and `"v"` (value):

| Type | Value fields | Example |
|------|-------------|---------|
| `verbatim` | `{"d":"docId","s":[start,end]}` | `{"t":"verbatim","v":{"d":"doc001","s":[0,100]}}` |
| `summary` | `{"d":"docId","s":[start,end]}` | `{"t":"summary","v":{"d":"doc001","s":[0,100]}}` |
| `extracted` | `{"d":"docId","s":[start,end],"m":[name,version]}` | `{"t":"extracted","v":{"d":"doc001","s":[0,100],"m":["gpt-4","2024-06"]}}` |
| `derived` | `{"f":"fromNodeId","r":"ruleId"}` | `{"t":"derived","v":{"f":"n001","r":"rule-merge"}}` |
| `asserted` | `{"b":"principal"}` | `{"t":"asserted","v":{"b":"admin"}}` |

The `"m"` (model) field in `extracted` is **required** — this constraint is
enforced in the parser and is non-negotiable. An `extracted` provenance
without a model reference is a spec violation.

## 6. Morpheme Table Negotiation

### Client Capability Declaration

A client declares Factum awareness by including `factum` in its
`initialize.params.capabilities`:

```json
{
  "method": "initialize",
  "params": {
    "capabilities": {
      "factum": {}
    }
  }
}
```

The server checks for the presence of `capabilities.factum`. If present,
the client is Factum-aware and the server sends the morpheme table. If
absent, the client is a vanilla MCP client and the server omits the
morpheme table (graceful degradation to string names).

This follows the standard MCP capability negotiation pattern — the same
mechanism used for `tools`, `resources`, `prompts`, etc.

### Server Response (Factum-aware client)

When the client declares `capabilities.factum`, the server includes
`factum_morphemes` in the `InitializeResult`:

```json
{
  "protocolVersion": "2025-06-18",
  "capabilities": { "tools": {...}, "resources": {...} },
  "serverInfo": { "name": "factum-mcp", "version": "0.1.0" },
  "factum_morphemes": [
    {"id": 0, "name": "instance-of", "kind": "Relation"},
    {"id": 1, "name": "shareholder-major", "kind": "Relation"},
    ...
  ]
}
```

### Server Response (vanilla MCP client)

When the client does **not** declare `capabilities.factum`, the server
omits `factum_morphemes` entirely. All morpheme references in compact
encoding use string names instead of u32 indices:

```json
{
  "protocolVersion": "2025-06-18",
  "capabilities": { "tools": {...}, "resources": {...} },
  "serverInfo": { "name": "factum-mcp", "version": "0.1.0" }
}
```

### MCP Extension Compliance

The `factum_morphemes` field in `InitializeResult` and the `factum`
capability in `ClientCapabilities` are **Factum-specific extensions** to
the MCP 2025-06-18 specification. The MCP spec defines `InitializeResult`
with `protocolVersion`, `capabilities`, and `serverInfo` only. Our
extensions add `factum_morphemes` as an additional response field and
`factum` as an additional client capability.

Because we use the standard capability declaration pattern, strict MCP
clients that reject unknown fields will simply not declare the `factum`
capability, and the server will fall back to string names. This is the
graceful degradation path — it is defined by the protocol, not left to
chance.

### Fallback / Degradation Behavior

When morpheme negotiation fails (client does not declare `factum`
capability, or an intermediary strips the extension), the compact form
**degrades gracefully**:

- `"1": "shareholder-major"` (string name) instead of `"1": 1` (u32 index)
- All other fields remain unchanged
- The receiver can detect string-vs-number in the `"1"` field and handle
  accordingly

This graceful degradation ensures that Factum compact form is functional
even with vanilla MCP clients that do not understand the morpheme extension.

## 7. Conformance Requirements

Implementations claiming compact-form conformance must:

1. **Round-trip**: `compact(deserialize(compact(x))) == compact(x)` for any
   valid Node `x`.
2. **Canonical equivalence**: `parse(canonical(x))` and
   `deserialize(compact(x))` must produce semantically identical Node structures
   (same id, predicate, validity, provenance, confidence, authority,
   permissions, deps).
3. **Literal disambiguation**: String literals starting with `?` or `@` must
   be encoded with embedded quotes. Decoding must correctly distinguish Var,
   Entity, and Literal.
4. **Morpheme fallback**: When morpheme indices are unavailable, fall back to
   string names without error.

### Conformance Vectors

Compact-form conformance vectors will be added to `spec/conformance/` as
`compact_basic.json` and `compact_edge_cases.json`. These should test:

- Basic compact round-trip (node with all 7 fields)
- Literal disambiguation (string `"?foo"`, string `"@bar"`, numeric `"42"`)
- Morpheme index vs string name fallback
- CompactValidity Forever vs Window vs open-ended
- All 5 provenance types

## 8. Form-Positioning Decision (Pending — Triggered by Issue #9)

**Status**: Decision pending. Heuristic token estimates suggest a repositioning
may be needed, but real tokenizer measurement (issue #9) is required before
making the spec-level change.

### The Finding

Heuristic token estimation (±15% of real o200k_base) revealed:

| Form | Bytes (5 nodes) | Est. tokens | Token reduction vs JSON |
|------|-----------------|-------------|-------------------------|
| Compact JSON | ~350 | ~350 | −12% |
| Canonical S-expr | 649 | ~134 | −66% |

The canonical form — designed for hashing and round-trip correctness — is
more token-efficient than the compact form, which was designed for transport
economy. This is because BPE tokenizers split JSON delimiters (`{`, `}`, `"`,
`:`) into individual tokens, while S-expression parentheses and whitespace are
frequently merged with adjacent tokens.

### Proposed Decision (Post-Issue-#9)

If real tokenizer measurement confirms the token gap:

1. **Rewrite compact form positioning**: from "LLM transport format" to
   "storage / service-to-service format" (byte-optimal, not token-optimal).
2. **Add `preferred_form` to `capabilities.factum`**:

```json
{
  "capabilities": {
    "factum": {
      "preferred_form": "canonical"
    }
  }
}
```

3. **Server behavior**: When `preferred_form` is `"canonical"`, the server
   returns `factum_query` results in canonical S-expression text (not compact
   JSON). When absent or `"compact"`, the server returns compact JSON (current
   behavior).

This is the first revision to the compact-form spec (v0.1-draft → v0.1-draft.1
if the decision is made). The revision will be documented in the changelog
and flagged as a breaking semantic change (return format negotiation, not
wire format change).
