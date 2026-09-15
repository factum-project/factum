# Confidence Calibration: Academic Research & Recommendations

> Research synthesis conducted 2026-09-15.  
> Triggered by: agent feedback ("置信度标注，我会糊弄") + user request for  
> "科学的又是合理的" confidence scoring.

---

## 1. Problem Statement

Factum's `:conf` field is an `f32` in [0,1] that participates in query-time
filtering (`min_conf`) and arbitrates trust. The current default is **1.0**
(`Confidence::default()`), and `factum_assert` inherits this default when the
caller omits `:conf`.

The problem is twofold:

1. **Agents cannot self-calibrate.** As one reviewing LLM candidly stated:
   "让我给一条知识标置信度 0.85，从内部视角看，这个数字接近虚构。"  
   LLMs can distinguish evidence *categories* (verbatim vs inferred) but
   cannot reliably produce calibrated *numeric* scores.

2. **The default of 1.0 is the worst possible default.** It signals certainty
   where none exists, undermines `min_conf` filtering, and creates false trust
   in arbitration.

This document reviews academic literature on LLM confidence calibration and
proposes a provenance-based default confidence mapping that replaces agent
self-assessment with evidence-category-based defaults.

---

## 2. Literature Review

### 2.1 Verbalized Confidence (Tian et al. 2023)

**Paper**: Tian, K., Mitchell, E., Yao, H., Manning, C.D., & Finn, C. (2023).  
*Just Ask for Calibration: An Approach for Improving and Calibrating LLM
Accuracy and Confidence*. Stanford University.

**Key findings**:
- RLHF-tuned models' **verbalized confidence** (asking "How confident are you?")
  is **better calibrated** than raw token probabilities.
- Verbalization reduces Expected Calibration Error (ECE) by approximately **50%**
  compared to token-probability-based confidence.
- However, models remain **systematically overconfident** — verbalized scores
  are inflated, especially for wrong answers.
- The insight: models "know" more than their raw logit probabilities suggest,
  but they don't "know" as much as they claim.

**Relevance to Factum**: When an agent sets `:conf 0.85`, this is a verbalized
confidence score. Tian et al.'s research shows it carries *some* signal (better
than random), but is systematically inflated. The agent's confession of
"fabrication" is academically validated — the number is not meaningless, but
it is unreliable.

### 2.2 Sample Consistency Calibration (Lyu et al. AAAI 2025)

**Paper**: Lyu, Z. et al. (2025). *Calibrating LLMs with Sample Consistency*.
AAAI 2025.

**Key findings**:
- Three consistency measures derived from multiple sampled generations:
  1. **Agreement** — fraction of samples that agree with the majority answer
  2. **Entropy** — Shannon entropy across the distribution of sampled answers
  3. **First-Second Distance (FSD)** — semantic distance between top-1 and top-2 answers
- Sample consistency **outperforms** both verbalized confidence and token
  probability as a calibration signal.
- **Instruction tuning makes calibration harder** — RLHF models are less
  calibrated than base models. This is a direct contradiction of Tian et al.'s
  optimism about verbalized confidence.
- Model scaling and larger sample sizes enhance calibration.

**Uncertainty proxy hierarchy** (from a related PMC study):
| Method | ROC AUC | Notes |
|--------|---------|-------|
| Sample Consistency (SC) | 0.68–0.79 | Best; but requires N≥5 samples |
| Token-Level Probability (TLP) | 0.71–0.87 | Good for base models; degraded by RLHF |
| Confidence Elicitation (CE) | 0.52–0.68 | Verbalized; consistently overestimates |

**Relevance to Factum**: Sample consistency is the most accurate confidence
estimation method, but it is expensive (N generations per assertion). Factum's
use case — real-time knowledge insertion during agent workflows — cannot afford
5x generation overhead. This method is better suited for offline batch
calibration or future "confidence audit" tools.

### 2.3 Clinical LLM Confidence Benchmarking (JMIR 2025)

**Study**: *Benchmarking Confidence and Accuracy of Large Language Models in
Clinical Settings*. JMIR Medical Informatics, 2025.

**Key findings**:
- **Inverse correlation** between mean confidence and accuracy (r = −0.40,
  P = .001) — worse-performing models exhibit *paradoxically higher* confidence.
- Token probability **outperforms** verbalized confidence for predicting
  accuracy: AUROC 0.71–0.87 (token) vs 0.52–0.68 (verbalized).
- Self-reported confidence ECE ranges from 0.06 to 0.127 across models.
- Clinical domain amplifies overconfidence — models are most overconfident
  in high-stakes scenarios where calibration matters most.

**Relevance to Factum**: The inverse correlation (r = −0.40) is the strongest
empirical evidence that **agent self-reported confidence is anti-correlated
with actual accuracy** in domain-specific tasks. This directly supports the
case for replacing self-assessment with provenance-based defaults.

### 2.4 CCPS — Perturbed Representation Stability (EMNLP 2025)

**Paper**: *Calibrating LLM Confidence by Probing Perturbed Representation
Stability (CCPS)*. EMNLP 2025.

**Key findings**:
- Applies adversarial perturbations to the model's final hidden states and
  measures output stability.
- Reduces ECE by approximately **55%** across 8B–32B parameter models.
- Does not require multiple generations (unlike sample consistency).
- But requires access to model internals — not available through API-only
  access (which is how most agents interact with LLMs).

**Relevance to Factum**: Not directly applicable (requires internal model
access), but validates that **representation stability correlates with
confidence** — a signal that provenance type (which approximates extraction
difficulty) can proxy for.

### 2.5 Knowledge Graph Trust Tiers

**Source**: Multiple production KG system designs (Neo4j, Amazon Neptune,
enterprise knowledge graph architectures).

**Key findings — trust tier systems**:

| Tier | Confidence Band | Source Type | Example |
|------|----------------|-------------|---------|
| Gold | 0.95–1.0 | Deterministic mappings, reviewer-approved | Ontology inference, human-verified |
| Silver | 0.75–0.90 | LLM-extracted from authoritative sources | SEC filing → entity extraction |
| Bronze | 0.40–0.70 | LLM-extracted from non-authoritative sources | News article, Wikipedia |
| Quarantine | 0.10–0.30 | Unverified, low-quality, or conflicting | Social media, anonymous |

**Source Provenance Score** (composite metric, 6 components):
1. Cryptographic chain of custody (hash verification)
2. Authoritative entity resolution (known entity vs ambiguous)
3. Primary source multiplier (original vs secondary reporting)
4. Temporal freshness decay function (exponential decay)
5. Multi-source corroboration vector (N independent sources → higher score)
6. Methodological transparency index (extraction method documented)

**Relevance to Factum**: The Gold/Silver/Bronze/Quarantine tier system maps
almost perfectly to Factum's `Provenance` enum. The Source Provenance Score's
6 components are partially captured by Factum's existing fields: provenance
type (components 1, 6), authority (components 2, 3), validity (component 4).
Components 5 (corroboration) is a future enhancement.

### 2.6 STALE Benchmark (HKUST NLP, 2025)

**Paper**: *STALE: Benchmarking LLM Agents on Outdated Memory Detection*.
HKUST NLP Group, 2025.

**Key findings**:
- 400 expert-validated conflict scenarios (1,200 evaluation queries).
- Tests three capabilities: State Resolution, Premise Resistance, Implicit
  Policy Adaptation.
- **Best model achieves only 55.2% accuracy** — agents are nearly as likely
  to use outdated information as to reject it.
- CUPMem prototype (explicit state adjudication) shows promising improvement.
- The problem is not storage — it's **agents' inability to assess when stored
  knowledge conflicts with new information**.

**Relevance to Factum**: STALE validates Factum's core value proposition —
structured provenance and temporal validity exist precisely to solve the
problem that agents cannot self-detect stale memories. But it also implies
that `:conf` alone cannot solve conflict detection — it must work with
`validity` and `provenance` in concert. The 55.2% accuracy means that without
structured metadata, agents will use wrong information ~45% of the time.

---

## 3. Synthesis: Three Confidence Estimation Paradigms

| Paradigm | Method | Accuracy (AUROC) | Cost | Available to Agents? |
|----------|--------|-------------------|------|---------------------|
| **Verbalized** | Ask model "how confident?" | 0.52–0.68 | 1 query | Yes (current Factum approach) |
| **Token Probability** | Use logit of generated token | 0.71–0.87 | 1 query | No (API doesn't expose logits) |
| **Sample Consistency** | N generations, measure agreement | 0.68–0.79 | N queries (≥5) | Expensive; not real-time |
| **Provenance-Based** | Map evidence type → default band | N/A (heuristic) | 0 queries | **Yes (proposed)** |

**Key insight**: The provenance-based approach does not try to measure the
model's internal confidence (which is unreliable). Instead, it uses the
*structural properties of the evidence* — which are deterministic and
auditable — as a proxy for confidence.

This is analogous to the KG trust tier approach: instead of asking "how
confident is this extraction?", we ask "what *type* of evidence is this?"
and assign a confidence band based on the evidence type's historical
reliability.

---

## 4. Recommendation: Provenance-Based Default Confidence (Revised)

> **Revision note (2026-09-15, post-review)**: This section was substantially
> revised after a 20-round expert review. The original version (v0.2) presented
> the point values as "scientifically-grounded defaults." The reviewer correctly
> identified that literature supports the *ordering* (verbatim > extracted >
> summary > asserted) but not the *point values* — no paper can distinguish
> whether 0.75 is more "scientific" than 0.80 for Summary. The revised version
> explicitly frames these as **policy constants informed by literature**, not
> scientific conclusions. Two additional mechanisms (band clipping, Derived
> decay) and one strategic upgrade (empirical reliability table, M3) were added.

### 4.1 Honesty Statement

**What the literature supports:**
- The *ordering* of confidence by evidence type: verbatim > extracted > summary
  > asserted. This is supported by KG trust tier conventions and the general
  finding that mechanical extraction is more reliable than inference.
- The claim that agent verbalized confidence is systematically inflated (Tian
  et al. 2023, JMIR 2025).
- The claim that provenance type is a usable *proxy* for confidence (KG trust
  tier systems in production).

**What the literature does NOT support:**
- The specific point value 0.80 for Extracted (vs 0.75 or 0.85). No paper
  measured "the correct default confidence for LLM-extracted knowledge from
  documents."
- The specific difference between Summary (0.75) and Extracted (0.80). This
  0.05 gap is a judgment call, not a measured quantity.

**Therefore**: The values in §4.2 are **policy constants** — chosen based on
KG trust tier conventions and the relative ordering from literature, but
ultimately arbitrary within their bands. They are explicitly designed to be
revised when empirical data becomes available (see §4.6 Empirical Reliability
Table). Framing them as "scientifically-grounded" would be the exact kind of
false certainty that Factum exists to prevent.

### 4.2 The Mapping (Policy Constants)

Map `Provenance` variant → default `Confidence` when the caller does not
provide an explicit `:conf` value:

| Provenance | Default `:conf` | Allowed agent range | Exceeding range requires | KG tier analog |
|------------|----------------|---------------------|-------------------------|----------------|
| `Verbatim` | **0.95** | [0.90, 0.98] | Exceeding rejected/logged | Gold (0.95–1.0) |
| `Extracted` | **0.80** | [0.60, 0.90] | >0.90 needs corroboration | Silver (0.75–0.90) |
| `Summary` | **0.75** | [0.50, 0.85] | >0.85 needs corroboration | Silver–Bronze boundary |
| `Asserted` | **0.60** | [0.30, 0.80] | >0.80 needs verified principal | Bronze (0.40–0.70) |
| `Derived` | **min(deps) × rule_reliability** | N/A (computed) | See §4.5 | Gold (if rule verified) |

**Literature basis for the ordering** (not the point values):
- Verbatim > Extracted: KG Gold > Silver convention; mechanical extraction
  has lower error rates than semantic extraction.
- Extracted > Summary: Summarization is lossy by definition; compounded
  uncertainty (extraction + condensation).
- Extracted > Asserted: JMIR 2025 (r=−0.40) shows self-reported confidence is
  anti-correlated with accuracy; a fixed default based on evidence type is
  more reliable than agent self-assessment.
- Asserted at 0.60 (not 1.0): Human assertions carry error; KG Bronze tier
  convention for unverified assertions. 1.0 is reserved for cryptographic
  provenance (future `Derived` with Lean proofs).

### 4.3 Band Clipping: Don't Discard Agent Signal, Constrain It

The original proposal (v0.2 §4.3-4.4) had a known weakness: the default was
"a floor, not a ceiling" — agents could set `:conf 0.99` for speculative
claims with no guardrail. The reviewer correctly identified this as
incomplete.

Tian et al. (2023) showed that verbalized confidence is **systematically
inflated but carries signal** (ECE reduced ~50% vs token probabilities).
Completely discarding the agent's self-assessment wastes this signal. The
solution is **band clipping**: each provenance type has an allowed range.
Agent self-assessment within the range is accepted; values exceeding the
range require additional evidence or are rejected.

**How band clipping works**:

1. Agent provides `:conf 0.75` for an Extracted node → within [0.60, 0.90]
   → accepted. The agent's relative judgment ("this extraction is somewhat
   uncertain") is preserved.

2. Agent provides `:conf 0.95` for an Extracted node → exceeds [0.90] →
   system checks for corroboration (same canonical predicate asserted by
   ≥2 independent `(principal, model)` pairs). If corroboration exists,
   accept 0.95. If not, reject or clamp to 0.90 with a warning.

3. Agent provides `:conf 0.99` for an Asserted node → exceeds [0.80] →
   system checks if the principal is cryptographically verified (future
   feature). If not, reject or clamp to 0.80 with a warning.

4. Agent provides `:conf 0.40` for an Extracted node → within [0.50] lower
   bound → this is *below* the allowed range. If the agent is this uncertain,
   it probably shouldn't insert the node at all. System suggests: "confidence
   0.40 is below the Extracted range [0.50, 0.90]; consider not inserting
   this knowledge."

**Implementation note**: Band clipping requires store queries (corroboration
check) that are not in the current `factum_assert` path. The clipping rules
are documented here as policy; code enforcement is planned for M2+.
Initially, `factum_assert` will apply the **default** (when `:conf` is
omitted) and emit a **warning** (when `:conf` exceeds the band), but will
not **reject** out-of-band values.

### 4.4 Corroboration Counting (with Independence Deduplication)

Corroboration — the same fact asserted by multiple independent sources — is
the strongest signal for raising confidence above the default band. Factum's
content-addressed node IDs (canonical predicate hash) make corroboration
naturally detectable: two nodes with the same canonical predicate are
assertions about the same fact.

**Independence requirement**: The same model asserting the same fact twice
is **not** corroboration — it carries the same biases. Corroboration must
be counted by unique `(principal, model)` pairs:

- User A + claude-sonnet-4 asserts X → 1 independent source
- User B + gpt-4o asserts X → 2 independent sources
- User A + claude-sonnet-4 asserts X again → still 1 (content-addressed ID
  collision → insert returns AlreadyExists)

Content-addressed IDs naturally prevent the same `(principal, model)` from
double-counting — the second insert is rejected. But different principals
using the same model, or the same principal using different models, both
count as independent sources.

**Corroboration threshold**: ≥2 independent `(principal, model)` pairs for
the same canonical predicate → allows confidence up to 0.95 regardless of
provenance type. ≥3 pairs → allows up to 0.98.

This mechanism is where Factum's mandatory `model` reference on `Extracted`
provenance pays off again: without it, corroboration counting would be
impossible because there would be no way to detect "same model, same bias."

**Implementation**: Planned for M2+ alongside band clipping. Requires a
`by_predicate_hash` index or `all_active()` scan filtered by canonical
predicate.

### 4.5 Derived Confidence: Multiplicative Decay

The original proposal (v0.2 §4.2) said Derived should "inherit" the source
node's confidence. The reviewer correctly identified that naive inheritance
is epistemologically wrong: a 10-step derivation chain would still have
conf=0.80 at the end, implying no uncertainty accumulation.

**Revised formula**:

```
conf_derived = min(active_deps' conf) × rule_reliability
```

Where:
- `active_deps` = dependency nodes with `status == Active` (retracted deps
  trigger cascade retraction, so they shouldn't be in the computation)
- `rule_reliability` = confidence in the derivation rule itself:
  - **0.95** (default for unverified rules — rules can be misapplied)
  - **1.0** (for formally verified rules, e.g., Lean proofs — future work)

**Decay behavior**:
- 1 step: 0.80 × 0.95 = 0.76
- 5 steps: 0.80 × 0.95⁵ = 0.615
- 10 steps: 0.80 × 0.95¹⁰ = 0.476

This geometric decay is defensible: longer derivation chains accumulate
more uncertainty. The `rule_reliability` parameter provides a clean
interface for future Lean verifier integration — verified rules don't
decay, unverified rules decay 5% per step.

**Multi-source deps**: Using `min()` (not average or weighted) is
intentional — a derivation chain is only as strong as its weakest link.
If node A (conf=0.95) and node B (conf=0.40) jointly derive node C,
C's confidence is limited by B's uncertainty: 0.40 × 0.95 = 0.38.

**Edge case**: If all deps are retracted, the Derived node should already
be cascade-retracted (existing `deps_rev` mechanism). If it somehow isn't,
conf should be set to 0.0 — derivation from invalid premises is worthless.

**Implementation**: Requires store access at insertion time to look up
dep nodes' confidence. `factum_assert` always sets `Asserted` provenance,
so this primarily affects the `factum_insert` path. The `factum_insert`
handler already has store access, so the lookup is straightforward.

### 4.6 Empirical Reliability Table (M3 — The Strategic Upgrade)

The preceding sections describe a **policy-based heuristic**: fixed constants
informed by literature but not measured. The reviewer proposed the most
important upgrade: turn `:conf` from "a guess at write time" into "a
measurable statistic."

**Core insight**: Factum's `Extracted` provenance **mandates** a model
reference (`ModelRef { name, version }`). This means the store naturally
accumulates data answering: *"Which model extracted which type of
knowledge, and how often was it later retracted/corrected?"*

**Proposed mechanism**:

1. **Data collection** (already exists): Every retract operation is
   recorded as a soft-delete (`status = Retracted`). The store already
   retains the full node including its `provenance` and `model` fields.

2. **Aggregation query** (new, M3): `compute_reliability_table()` scans
   all nodes, groups by `(provenance_type, model_name)`, and computes:

   ```rust
   struct ReliabilityStats {
       total: usize,
       active: usize,
       retracted: usize,
       observed_accuracy: f32,  // active / total
   }
   ```

3. **Default value auto-switch** (new, M3): `default_confidence_for_provenance()`
   checks if empirical data exists for the given `(provenance_type, model)`
   pair. If N ≥ 10 (cold-start threshold), it returns the observed accuracy.
   Otherwise, it falls back to the §4.2 policy constants.

4. **Retract reason classification** (new, M3): Not all retracts mean
   "wrong." A node might be retracted because it's outdated (CEO changed),
   superseded (better data arrived), or simply cleaned up. Only `error`
   and `superseded` retracts should count against the model's reliability
   score. This requires adding an optional `reason` parameter to the
   retract operation:
   - `"error"` — extraction was factually wrong
   - `"outdated"` — fact was correct but is no longer current
   - `"superseded"` — replaced by a more accurate node
   - `"cleanup"` — user administrative cleanup

   Only `"error"` and `"superseded"` reduce the model's reliability score.

**Why this is the killer feature**: Calibration, by definition, means
"predicted probability matches observed frequency." A system that uses
fixed constants is *inspired* by calibration literature. A system that
measures its own accuracy and feeds it back into default confidence is
*actually calibrated*. This is the differentiator that no competing agent
memory system has — and it's only possible because Factum mandates
provenance with model references.

**M3 roadmap placement**: See ROADMAP.md §M3 "Confidence Feedback Loop."

### 4.7 Caller Override (Revised)

With band clipping (§4.3), the caller override semantics change from the
original v0.2 proposal:

```scheme
; Agent extracts from SEC filing — no :conf provided
(assert "(revenue @ACME-CORP 23050000000)" :by "claude-sonnet-4")
; → conf defaults to 0.80 (Extracted policy constant)

; Agent extracts and is less certain than typical (e.g., ambiguous text)
(assert "(ceo-of @FOUNDER-1 @ACME-CORP)" :by "claude-sonnet-4" :conf 0.65)
; → conf = 0.65 (within Extracted band [0.60, 0.90], accepted)

; Agent extracts and is very certain (multi-source corroboration exists)
(assert "(revenue @ACME-CORP 23050000000)" :by "claude-sonnet-4" :conf 0.95)
; → conf = 0.95 (exceeds Extracted band [0.90])
; → M2+ system checks corroboration; if ≥2 independent sources, accept
; → M2+ if no corroboration, clamp to 0.90 with warning
; → Current: accepted with warning (no enforcement yet)

; Agent asserts with low confidence (should probably not insert)
(assert "(may-acquire @ACME-CORP @TARGET-CO)" :by "user" :conf 0.25)
; → conf = 0.25 (below Asserted band [0.30])
; → system suggests: "consider not inserting this knowledge"
```

### 4.8 What This Does NOT Do (Revised)

- **Does not compute calibrated confidence in real time.** True calibration
  (sample consistency, CCPS) is too expensive for real-time agent workflows.
  The empirical reliability table (§4.6) provides *retrospective* calibration,
  not *prospective*.
- **Does not enforce band clipping in v0.1.** The current implementation
  applies the default and emits warnings; enforcement (reject/clamp) is M2+.
- **Does not replace `authority`.** Authority (source trustworthiness) and
  confidence (claim correctness) remain separate axes.
- **Does not migrate existing nodes.** Nodes already in the store with
  conf=1.0 retain their value. Only new insertions are affected. This is
  consistent with Factum's immutability principle (retract + insert, no
  in-place update).

---

## 5. Implementation Plan (Revised)

### 5.1 Code Change: `calibration.rs` (new file)

**File**: `crates/factum-core/src/calibration.rs` (new)

Placing the mapping in a dedicated `calibration.rs` (rather than `types.rs`)
signals that these are **adjustable policy constants**, not core data type
definitions. Future configuration-file loading would only require changing
this file's implementation, not the API.

```rust
//! Confidence calibration policy.
//!
//! These values are **policy constants** informed by academic literature
//! and KG trust tier conventions, NOT scientific measurements. The
//! literature supports the *ordering* (verbatim > extracted > summary >
//! asserted) but not the *point values*. Values will be revised when
//! empirical data becomes available (see M3: Empirical Reliability Table).
//!
//! See `docs/confidence-calibration-research.md` §4 for full rationale.

use crate::types::{Confidence, Provenance};

/// Default rule reliability for unverified derivation rules.
/// Formally verified rules (Lean proofs) will use 1.0 (future work).
pub const DEFAULT_RULE_RELIABILITY: f32 = 0.95;

/// Provenance-based default confidence (policy constants).
///
/// When the caller does not provide an explicit `:conf` value, the system
/// assigns a default based on the evidence type (provenance variant).
pub fn default_confidence_for_provenance(p: &Provenance) -> Confidence {
    match p {
        Provenance::Verbatim { .. } => Confidence(0.95),
        Provenance::Extracted { .. } => Confidence(0.80),
        Provenance::Summary { .. } => Confidence(0.75),
        Provenance::Asserted { .. } => Confidence(0.60),
        // Derived: cannot compute without store access (needs dep lookup).
        // Callers with store access should use derived_confidence() instead.
        // This fallback is conservative (same as Extracted).
        Provenance::Derived { .. } => Confidence(0.80),
    }
}

/// Compute confidence for a Derived node using multiplicative decay.
///
/// `conf_derived = min(active_deps' conf) × rule_reliability`
///
/// - Uses min() because a derivation chain is only as strong as its weakest link.
/// - `rule_reliability` defaults to 0.95 (rules can be misapplied).
///   Formally verified rules (future Lean verifier) use 1.0 (no decay).
/// - If all deps are retracted, returns Confidence(0.0) — derivation from
///   invalid premises is worthless. (Cascade retraction should have already
///   handled this, but this is a safety net.)
pub fn derived_confidence(
    dep_confidences: &[Confidence],
    rule_reliability: f32,
) -> Confidence {
    if dep_confidences.is_empty() {
        return Confidence(0.0);
    }
    let min_conf = dep_confidences.iter()
        .map(|c| c.0)
        .fold(f32::INFINITY, f32::min);
    Confidence(min_conf * rule_reliability)
}

/// Allowed confidence range for band clipping (§4.3).
///
/// Returns (lower, upper) bounds for agent self-assessment.
/// Values outside this range require additional evidence (corroboration,
/// verified principal) or should be reconsidered.
pub fn confidence_band(p: &Provenance) -> (f32, f32) {
    match p {
        Provenance::Verbatim { .. } => (0.90, 0.98),
        Provenance::Extracted { .. } => (0.60, 0.90),
        Provenance::Summary { .. } => (0.50, 0.85),
        Provenance::Asserted { .. } => (0.30, 0.80),
        Provenance::Derived { .. } => (0.0, 1.0), // computed, not self-assessed
    }
}
```

### 5.2 Code Change: `Confidence::default()`

**File**: `crates/factum-core/src/types.rs`, line 238-242.

**Current**:
```rust
impl Default for Confidence {
    fn default() -> Self {
        Self(1.0)
    }
}
```

**Proposed**: Change to `Self(0.60)` — the most conservative provenance-agnostic
default (matching `Asserted`, the least certain non-Derived provenance type).
This ensures that even if a code path bypasses `default_confidence_for_provenance()`,
the fallback is conservative rather than falsely certain.

**Impact**: `Node::new()` and `parser.rs` both use `Confidence::default()`.
Tests that assert `confidence == 1.0` on newly created nodes will need
updating. `Confidence::certain()` (which returns 1.0) is unchanged — it
remains available for explicit "certain" assertions.

### 5.3 Code Change: `factum_assert` default confidence

**File**: `crates/factum-mcp/src/handler.rs`, `tool_assert()` method.

**Current** (line 622-624):
```rust
if let Some(conf) = params.confidence {
    node.confidence = Confidence(conf);
}
// If not provided: stays as Confidence::default() = 1.0  ← THE PROBLEM
```

**Proposed**:
```rust
if let Some(conf) = params.confidence {
    node.confidence = Confidence(conf);
    // M2+: check band clipping, emit warning if out of range
} else {
    node.confidence = calibration::default_confidence_for_provenance(&node.provenance);
}
```

### 5.4 `min_conf` Interaction Check

**Code audit result** (grep `min_conf` across all `.rs` files):
- `query.rs:52`: `QueryOptions` default `min_conf: Confidence(0.0)` — safe,
  defaults to no filtering.
- `handler.rs:252-253`: `factum_query` `min_confidence: Option<f32>` — safe,
  defaults to None → no filter.
- `handler.rs:377-378`: `factum_lookup` same pattern — safe.

**No hardcoded `min_conf: 0.70` in code.** The risk is only in documentation
examples. Action: update `docs/confidence-calibration.md` §"Query-Time
Calibration" to note that `min_conf: 0.70` will filter out `Asserted` nodes
(default 0.60).

**Existing nodes**: Nodes already in the store with conf=1.0 are NOT
migrated. Only new insertions use the new defaults. This is consistent with
Factum's immutability principle.

### 5.5 Documentation Updates

1. `docs/confidence-calibration.md`: Update with band clipping table, honest
   framing, Derived decay formula, min_conf interaction note.
2. `CHANGELOG.md`: Record the revised approach.
3. `ROADMAP.md`: Add M3 "Confidence Feedback Loop" section.

### 5.6 Test Changes

New tests needed (in `calibration.rs`):
- `test_default_confidence_for_each_provenance` — table-driven test for all
  5 provenance variants
- `test_derived_confidence_decay` — verify multiplicative decay:
  single dep, multi-dep (min), empty deps (0.0), rule_reliability=1.0
- `test_confidence_band_ranges` — verify (lower, upper) for each provenance

Updated tests:
- `test_node_builder` (`types.rs:687`) — update assertion from
  `Confidence(1.0)` to `Confidence(0.60)` or use explicit `.with_confidence()`
- `test_assert_success` (`handler.rs` tests) — update to expect
  `Confidence(0.60)` for Asserted nodes without explicit `:conf`

---

## 6. Risk Assessment (Revised)

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Existing tests break (asserted `conf == 1.0`) | High | Low | Update tests; expected breaking change |
| Agents that relied on default 1.0 now get 0.60 | Medium | Medium | Desired behavior; agents should set `:conf` explicitly |
| `min_conf: 0.70` queries silently exclude Asserted nodes | Medium | Medium | Document; no hardcoded thresholds in code |
| Band clipping not enforced (M2+ gap) | High | Low | Warnings emitted; rejection deferred to M2+ |
| Empirical reliability table (M3) retract semantics | Medium | Medium | Need retract reason classification; "error" vs "outdated" |
| Policy constants are "wrong" (too high or too low) | Medium | Low | Explicitly framed as adjustable; M3 auto-corrects with data |
| Users surprised by behavioral change | Medium | Low | CHANGELOG as breaking change; calibration guide updated |

---

## 7. References

1. **Tian, K. et al.** (2023). "Just Ask for Calibration: An Approach for
   Improving and Calibrating LLM Accuracy and Confidence." Stanford University.
   — Verbalized confidence reduces ECE ~50% vs token probabilities, but still
   systematically overconfident.

2. **Lyu, Z. et al.** (2025). "Calibrating LLMs with Sample Consistency."
   AAAI 2025. — Three consistency measures (agreement, entropy, FSD)
   outperform post-hoc approaches. Instruction tuning degrades calibration.

3. **JMIR Medical Informatics** (2025). "Benchmarking Confidence and Accuracy
   of Large Language Models in Clinical Settings." — Inverse correlation
   between confidence and accuracy (r=−0.40). Token probability AUROC 0.71-0.87
   vs verbalized AUROC 0.52-0.68.

4. **EMNLP 2025**. "Calibrating LLM Confidence by Probing Perturbed
   Representation Stability (CCPS)." — Adversarial perturbation of hidden
   states reduces ECE ~55%. Requires model internals.

5. **HKUST NLP** (2025). "STALE: Benchmarking LLM Agents on Outdated Memory
   Detection." — 400 scenarios, best model 55.2% accuracy. Validates need
   for structured provenance and temporal validity.

6. **Knowledge Graph Trust Tiers** — Production KG systems (Neo4j, Amazon
   Neptune) use Gold/Silver/Bronze/Quarantine tiers with confidence bands
   (1.0 for deterministic, 0.4-0.95 for LLM-extracted, 1.0 for reviewer-
   approved). Source Provenance Score: 6-component composite metric.

7. **PMC Study on Uncertainty Proxies** — Sample consistency (SC) > token-
   level probability (TLP) > confidence elicitation (CE). SC by sentence
   embedding ROC AUC 0.68-0.79. Verbalized confidence consistently
   overestimates model confidence.

---

## 8. Version History

- v0.3 (2026-09-15): Post-review revision. Framed point values as "policy
  constants" not "scientific conclusions" (§4.1 Honesty Statement). Added
  band clipping mechanism (§4.3). Added corroboration counting with
  `(principal, model)` deduplication (§4.4). Replaced Derived "inherit"
  with multiplicative decay formula (§4.5). Added Empirical Reliability
  Table as M3 strategic upgrade (§4.6). Revised implementation plan with
  `calibration.rs` file, `min_conf` audit, and band clipping tests (§5).
- v0.2 (2026-09-15): Academic research synthesis + provenance-based default
  confidence proposal. Added 7 academic references.
- v0.1 (2026-09-14): Initial calibration guide for v0.1.0 release.
