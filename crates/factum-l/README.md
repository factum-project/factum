# factum-l

**Archived research experiment — measured, negative result.**

> factum-l tested whether structured knowledge can be compressed into
> low-dimensional vectors and reconstructed with high fidelity. The result
> is negative: open-set identifiers (entity names) cannot be losslessly
> compressed into fixed dimensions without a codebook. This is an
> information-theoretic barrier, not an engineering limitation.

## Deliverables

The experiment produced one production-ready component and two methodology
artifacts:

| Component | Status | Value |
|-----------|--------|-------|
| `SemEquiv` | **Production-ready** | Structured semantic equivalence comparison |
| Anti-cheat gates | Methodology | Measurement protocol for future encoders |
| Packed codec baseline | Reference | 24-dim structural packing (gate-passing, not compression) |

## SemEquiv — The Production-Ready Deliverable

`SemEquiv` provides auditable, structured semantic equivalence comparison
between Factum nodes. It uses 4-level calibrated scoring (not vector cosine):

| Level | Score | Condition |
|-------|-------|-----------|
| Exact | 1.0 | All fields match (except `id` and `note`) |
| MetadataDrift | 0.85 | Predicate fully matches, metadata differs |
| WrongValues | <=0.3 | Head + arg count + types match, values differ |
| Different | 0.0 | Head differs, arg count mismatches, or types incompatible |

Use cases: node deduplication, conflict detection, upsert verification.

```rust
use factum_l::SemEquiv;
use factum_core::morphemes::MorphemeRegistry;
use std::sync::Arc;

let sequiv = SemEquiv::new(Arc::new(MorphemeRegistry::with_seeds()));
let result = sequiv.compare(&node_a, &node_b);
println!("Score: {} ({:?})", result.score, result.level);
```

## Anti-Cheat Gates

Three gates prevent "memorization encoders" from gaming the benchmark:

1. **Dimension budget (bit comparison)**: vector_bits < canonical_bits / 3
2. **Latent space operation**: normalize + tiny noise applied to z
3. **Train/test split**: 50% of nodes held out for testing only

## Negative Result Analysis

### Information-Theoretic Barrier

Entity names are an open set — infinitely many possible strings. Fixed
dimensions store at most `dims * 32` bits. The packed codec packs the
first 12 characters per arg (base-67 encoding), which is **truncation,
not compression**. Entity names > 12 chars are permanently lost.

### Gate 1 Bit Comparison

After fixing Gate 1 to compare bits (not float count vs byte count):
- 24 dims * 32 bits = 768 bits
- ~111 canonical bytes * 8 = 888 bits
- Compression ratio: only 1.16x

### Named Args Not Encodable

The 24-dim budget has no space for named arg key/value pairs. Only
presence + type is encoded. Nodes with named args score 0.0 (Different).

### What Was Rejected

- **LLM-as-Codec**: LLM hallucination conflicts with refuse-to-guess
  principle; non-determinism breaks provenance chain
- **Neural encoder training**: information-theoretic barrier applies
  equally to learned encoders without a codebook
- **Symbol table codec**: codebook is data relocation, not compression

See `docs/packed-codec-design.md` for the full analysis.
