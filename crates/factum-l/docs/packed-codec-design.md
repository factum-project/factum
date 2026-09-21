# Packed Codec Design (Archived — Negative Result)

## Status: Archived

This design document records the structural packing codec experiment.
The experiment is complete. The result is negative: structured knowledge
cannot be losslessly compressed into fixed-dim vectors without a codebook,
due to the open-set nature of entity identifiers.

## What Was Built

A 24-dim structural packing codec (`packed_codec.rs`) that:
- Packs predicate head (morpheme index), arg types, and arg values into floats
- Uses base-67 character encoding (2 chars per float, 12 chars per arg)
- Employs scale-reference normalization recovery (dim[0]=1.0) to survive
  Gate 2's normalization + noise
- Passes Gate 1 (768 bits < 888 bits / 3 for benchmark nodes)
- Achieves 0.85 SemEquiv score on 9/10 nodes (MetadataDrift)
- Fails on nodes with named args (1/10) due to budget constraints

## Why It Was Archived

### Information-Theoretic Barrier

Entity names are an open set — infinitely many possible strings. Fixed
dimensions can store at most `dims * 32` bits of information. The codec
packs the first 12 characters of each entity name, which is **truncation,
not compression**. Entity names longer than 12 chars are permanently lost.

### Gate 1 Bit Comparison

After fixing Gate 1 to use bit comparison (vector_dim * 32 vs
canonical_bytes * 8), the compression ratio is only 1.16x — the codec
barely passes Gate 1 for short benchmark nodes and would fail for
longer canonical texts.

### Named Args Not Encodable

The 24-dim budget has no space for named arg key/value pairs. Only
presence + type is encoded. Nodes with named args score 0.0 (Different).

## Architecture (For Reference)

### Vector Layout (24 dims)

```
[0]      scale_ref       — always 1.0 (normalization recovery)
[1]      head_index      — morpheme registry index / max_morphemes
[2]      packed_args     — arg_count + 3 type codes (base-11)
[3..9]   arg1_chars      — 6 floats, 2 chars each = 12 chars max
[9..15]  arg2_chars      — 6 floats, 2 chars each = 12 chars max
[15..21] arg3_chars      — 6 floats, 2 chars each = 12 chars max
[21]     has_named       — 0.0 or 1.0 + type code / 100
[22]     confidence      — node confidence
[23]     authority       — node authority
```

### Character Encoding

- 66-char charset (A-Z, a-z, 0-9, -, ., (, ))
- Index 0 = sentinel (end of string)
- 2 chars per float: value = c1*67 + c2, stored as value/4488
- After normalization recovery: error ~2.4e-6 (90x margin)

## Feature Flag Design (Preserved For Future Reference)

The `transformer` feature flag pattern was designed to keep external
dependencies (reqwest, tokio) out of the default compile path. This
pattern should be reused when adding any external capability (HTTP
transport, cloud sync, vector indexing):

```toml
[features]
transformer = ["dep:reqwest", "dep:tokio"]
```

Default `cargo build` remains zero-network-dependency. CI does not
require external API keys. Embedded scenarios can use factum-core
without pulling in async runtime.

## Deliverables From This Experiment

1. **SemEquiv** (sequiv.rs) — production-ready, independently useful
2. **Anti-cheat gates** (benchmark.rs) — measurement methodology
3. **Packed codec baseline** (packed_codec.rs) — reference implementation
4. **Negative result analysis** — this document

## What Was NOT Built (And Should Not Be)

- LLM-as-Codec (HuggingFace API encode/decode) — rejected: LLM
  hallucination conflicts with refuse-to-guess principle, non-determinism
  breaks provenance chain
- Neural encoder training (Qwen2.5-7B + LoRA) — rejected: information-
  theoretic barrier applies equally to learned encoders without a codebook
- Symbol table codec — rejected: codebook is data relocation, not
  compression; total information (z + codebook) exceeds original
