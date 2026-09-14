# Authoring Factum-F for LLMs

This guide is for LLMs (via system prompt or few-shot examples) and for humans
configuring prompts that ask LLMs to generate Factum-F nodes. It covers the
constructs LLMs most commonly get wrong, the error classes they'll receive,
and self-correction patterns.

> **Dual purpose**: This document can be used as a system prompt appendix for
> LLM-based Factum-F generation, and can also be served as an MCP `prompt`
> resource for clients that request authoring guidance.

## Quick Reference: Node Structure

Every Factum-F node is a 7-tuple with this canonical structure:

```scheme
(node <id>
  :pred (<head> <arg>* <named-arg>*)
  :valid <validity>
  :src <provenance>
  :conf <0.0-1.0>
  :auth <0.0-1.0>
  :perm <tag>
  :deps [<id>*])
```

**Required fields**: `:pred` (the assertion). All other fields have defaults:
- `:valid` defaults to `forever`
- `:src` defaults to `(asserted "system")`
- `:conf` defaults to `1.0`
- `:auth` defaults to `0.5`
- `:perm` defaults to `public`
- `:deps` defaults to empty

## LLM Common Errors and Self-Correction

### 1. MissingPredField

**What happens**: LLM generates a node without `:pred`.

```scheme
; ❌ Wrong — no :pred field
(node n001 :conf 0.85 :src (asserted "admin"))

; ✅ Correct
(node n001 :pred (instance-of @ACME-CORP organization) :conf 0.85 :src (asserted "admin"))
```

**Self-correction**: If you receive `MissingPredField`, add a `:pred` field
with a predicate head and at least one argument.

### 2. NamedArgBeforePositional

**What happens**: LLM puts a named argument (`:key value`) before a positional
argument.

```scheme
; ❌ Wrong — positional @Y comes after named :x
(revenue @X :period #date(2024-01-01) @Y 1000)

; ✅ Correct — all positional args first, then named args
(revenue @X @Y 1000 :period #date(2024-01-01))
```

**Self-correction**: Reorder so all positional arguments come before any
`:keyword value` pairs. Named arguments must be last.

### 3. MissingModelRef

**What happens**: LLM uses `:src (extracted ...)` but forgets the model
reference. This is the most important error to get right — Extracted
provenance **must** carry model name and version.

```scheme
; ❌ Wrong — no model reference
:src (extracted "doc001" [0 100])

; ✅ Correct — model name and version included
:src (extracted "doc001" [0 100] (model "gpt-4" "2024-06"))
```

**Self-correction**: If you receive `MissingModelRef`, add `(model <name> <version>)`
after the span. The model should be the LLM that extracted this knowledge
(typically yourself).

### 4. UnbalancedParen

**What happens**: LLM generates mismatched parentheses.

**Self-correction**: Count opening and closing parentheses. Every `(` must
have a matching `)`. Use the parser error's line/col to locate the mismatch.

### 5. UnterminatedString

**What happens**: LLM starts a string with `"` but doesn't close it.

```scheme
; ❌ Wrong — string not closed
:pred (note @X "this is a note)

; ✅ Correct
:pred (note @X "this is a note")
```

**Self-correction**: Ensure every `"` has a matching closing `"`.

## Few-Shot Templates

### Template 1: Basic Entity Fact

```scheme
(node n001
  :pred (instance-of @ACME-CORP organization)
  :conf 0.99 :auth 0.95 :perm public
  :src (asserted "wikidata"))
```

### Template 2: Extracted Knowledge (LLM-generated)

```scheme
(node n002
  :pred (revenue @ACME-CORP 23050000000 :period #date(2024-01-01) :currency "USD")
  :conf 0.85 :auth 0.8 :perm confidential
  :src (extracted "earnings-report-2024" [120 350] (model "gpt-4" "2024-06")))
```

### Template 3: Derived Knowledge with Dependencies

```scheme
(node n003
  :pred (subsidiary-of @ACME-SUB @ACME-CORP :since #date(2001-03-15))
  :src (derived n001 "rule-subsidiary-merge")
  :deps [n001])
```

> **:deps semantics**: `:deps` lists nodes whose retraction should
> invalidate this node (cascade retraction). Only list nodes that this
> derivation directly depends on — listing unrelated nodes will cause
> false invalidation when they are retracted.

### Template 4: Temporal Fact with Validity Window

```scheme
(node n004
  :pred (ceo-of @FOUNDER-1 @ACME-CORP)
  :valid (window "2001-03-15T00:00:00Z" "2024-12-31T00:00:00Z")
  :conf 0.95 :auth 0.9 :perm internal
  :src (extracted "press-release" [0 500] (model "gpt-4" "2024-06")))
```

## Interpreting Query Results

When you query via `factum_query`, you receive results in compact form
(JSON with numeric tags). Here's how to read them:

### Compact Form Number Tags

| Tag | Field | Example |
|-----|-------|---------|
| `"0"` | Node ID | `"n001"` |
| `"1"` | Predicate head (morpheme name or numeric ID) | `"instance-of"` or `3` |
| `"2"` | Positional arguments | `["@ACME-CORP", "organization"]` |
| `"3"` | Named arguments (if any) | `[["since", "2001-03-15"]]` |
| `"4"` | Validity | `"forever"` or `{"f":"2024-01-01","u":"2024-12-31"}` |
| `"5"` | Provenance | `{"t":"asserted","v":{"b":"wikidata"}}` |
| `"6"` | Confidence (f32) | `0.99` |
| `"7"` | Authority (f32) | `0.95` |
| `"8"` | Permission tag (bitmask) | `1` (public), `4` (confidential) |
| `"9"` | Dependencies (if any) | `["n001"]` |

### Terms in Compact Form

Arguments use an untagged union — the first character tells you the type:
- Starts with `?` → Variable (e.g., `"?x"`)
- Starts with `@` → Entity reference (e.g., `"@ACME-CORP"`)
- Anything else → Literal (e.g., `"0.73"`, `"\"hello\""`, `"#date(2024-01-01)"`)

String literals containing `?` or `@` are escaped with embedded quotes:
`"\"?foo\""` is the string `?foo`, not a variable.

### Handling Ambiguous Results

If the query result includes `"ambiguous": true`, it means the conflict
arbitration policy could not uniquely resolve the answer. **Do not pick
a winner yourself.** Report to the user:

> "This query returned multiple conflicting results that could not be
> uniquely resolved. The following sources disagree:
> - [list the conflicting nodes with their provenance and confidence]
> Would you like to refine the query or change the conflict policy?"

This is a core Factum principle: **the system refuses to guess rather
than present a potentially wrong answer as certain.**

## Provenance Level Selection Guide

| Scenario | Provenance Level | Example |
|----------|-----------------|---------|
| LLM extracts from a document | `extracted` (must include model) | `(extracted "doc001" [0 100] (model "gpt-4" "2024-06"))` |
| LLM summarizes a longer passage | `summary` | `(summary "doc001" [0 500])` |
| LLM quotes text verbatim | `verbatim` | `(verbatim "doc001" [42 87])` |
| LLM derives from existing nodes | `derived` | `(derived n001 "rule-merge")` |
| Human/system directly asserts | `asserted` | `(asserted "admin")` |

## Numeric Guidelines

- **Financial amounts, percentages, quantities**: Always use decimal notation (`230.50`, `0.73`, `42`). These are parsed as `Dec(i128, u8)` — lossless, no floating-point error.
- **Confidence and authority**: Use float notation (`0.85`, `0.9`). These are subjective measures, not exact quantities.
- **Dates**: Use `#date(YYYY-MM-DD)` format.
- **Durations**: Use `#dur(Nd)` format (e.g., `#dur(30d)`).

## Best Practices for LLM Generation

1. **Always include `:src`**: Every knowledge node should declare where it
   came from. If the LLM generated it, use `extracted` with the model reference.
2. **Set realistic `:conf`**: Don't default to 1.0 unless you're certain.
   0.8-0.9 is appropriate for LLM-extracted knowledge. See the
   [Confidence Calibration Guide](confidence-calibration.md) for detailed
   tables mapping source types and extraction methods to recommended
   confidence and authority values.
3. **Use `:deps` for derived facts**: If a fact depends on other facts,
   list them in `:deps`. This enables cascade retraction.
4. **Prefer named args for optional fields**: `:period`, `:currency`,
   `:since` should be named args, not positional.
5. **Submit and self-correct**: You cannot parse Factum-F locally — submit
   via `factum_insert`. If you receive an error class (e.g.,
   `MissingPredField`, `UnbalancedParen`), fix per the table above and
   resubmit. This is the normal authoring loop: generate → submit →
   receive error class → fix → resubmit.
