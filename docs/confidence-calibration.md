# Confidence & Authority Calibration Guide

This guide helps LLMs and humans assign realistic `confidence` and `authority`
values to Factum nodes. These two fields are **subjective measures** — they
are `f32`, not `Dec`, because they do not participate in exact arithmetic.
But they **do** participate in conflict arbitration: `LatestWins` sorts by
authority (then validity recency), and `min_conf` filters results at query
time. Bad calibration undermines both.

## Core Principle

> **Calibrate against your sources, not your feelings.**

A confidence score answers: *"How likely is this specific assertion to be
factually correct, given where it came from?"* — not *"How confident do I
feel about it?"*

An authority score answers: *"How trustworthy is the source of this
assertion, independent of the specific claim?"* — not *"How much do I trust
this assertion?"*

## Confidence vs Authority — What's the Difference?

| Dimension | Confidence (`:conf`) | Authority (`:auth`) |
|-----------|----------------------|---------------------|
| **Scope** | Per-assertion | Per-source |
| **Answers** | "Is this specific claim correct?" | "Is this source reliable?" |
| **Set by** | The agent/person creating the node | The system/admin configuring source trust |
| **Default** | 1.0 (certain) | 0.5 (neutral) |
| **Query filter** | `min_conf` in QueryOptions | Not directly filtered; used in arbitration |
| **Arbitration role** | Not used in policy comparison | `LatestWins` and `HighestAuthority` sort by this |

**Key insight**: An LLM extracting from a high-quality document (high authority)
may still produce an incorrect extraction (lower confidence). Conversely, a
low-authority source making a verifiable claim may have high confidence.

## Calibration Tables

### Authority by Source Type

| Source | Authority | Example |
|--------|-----------|---------|
| Official regulatory filing (SEC, CSRC) | 0.95–1.0 | `(asserted "sec-edgar")` |
| Peer-reviewed publication | 0.90–0.95 | `(asserted "pubmed")` |
| Established news agency (Reuters, Xinhua) | 0.80–0.90 | `(asserted "reuters")` |
| Company official press release | 0.75–0.85 | `(asserted "acme-ir")` |
| Wikidata / structured knowledge base | 0.70–0.85 | `(asserted "wikidata")` |
| Wikipedia | 0.60–0.70 | `(asserted "wikipedia")` |
| User-edited wiki / forum | 0.40–0.60 | `(asserted "community-wiki")` |
| Social media post | 0.20–0.40 | `(asserted "twitter")` |
| Unverified blog | 0.10–0.30 | `(asserted "personal-blog")` |
| Unknown / anonymous | 0.05–0.15 | `(asserted "anon")` |

### Confidence by Extraction Method

| Method | Confidence | When to use |
|--------|------------|-------------|
| Verbatim quote (exact text match) | 0.95–1.0 | Source text is unambiguous |
| Direct extraction (clear statement) | 0.85–0.95 | "Revenue was $23.05B" → `(revenue ... 23050000000)` |
| Summarization (faithful condensation) | 0.70–0.85 | Condensing a paragraph into a single fact |
| Inference (multi-hop reasoning) | 0.50–0.75 | "Company X acquired Y" inferred from multiple paragraphs |
| Speculation / uncertain interpretation | 0.30–0.50 | "The CEO may resign" from tone analysis |
| Guess / weak signal | 0.10–0.30 | Don't insert — query the user instead |

### Confidence by Knowledge Category

| Knowledge type | Typical confidence | Notes |
|----------------|-------------------|-------|
| Numeric fact (revenue, headcount) | 0.85–0.95 | Verifiable, high-precision extraction |
| Entity relationship (subsidiary-of, ceo-of) | 0.80–0.90 | Usually explicitly stated |
| Temporal fact (founded-on, acquired-by) | 0.85–0.95 | Dates are unambiguous when present |
| Categorical (instance-of) | 0.90–0.98 | Binary classification, low ambiguity |
| Causal claim | 0.40–0.65 | Hard to verify from text alone |
| Predictive / forward-looking | 0.20–0.40 | Inherently uncertain; consider not inserting |
| Subjective assessment | 0.30–0.50 | "Strong quarter" — opinion, not fact |

## Practical Examples

### Example 1: Direct extraction from earnings report

```scheme
(node n001
  :pred (revenue @ACME-CORP 23050000000 :period #date(2024-01-01) :currency "USD")
  :conf 0.95 :auth 0.90 :perm public
  :src (extracted "earnings-2024-q1" [120 350]
         (model "claude-sonnet-4" "2025-01")))
```

**Why 0.95 confidence?** The number is explicitly stated in a financial
filing. The extraction is near-mechanical — the only risk is a digit
transposition, which is unlikely.

**Why 0.90 authority?** SEC filing — high trust, but not 1.0 because
filings can contain errors that are later amended.

### Example 2: Inferred subsidiary relationship

```scheme
(node n002
  :pred (subsidiary-of @ACME-SUB @ACME-CORP :since #date(2001-03-15))
  :conf 0.70 :auth 0.75 :perm internal
  :src (extracted "press-release-2001" [0 500]
         (model "claude-sonnet-4" "2025-01")))
```

**Why 0.70 confidence?** The press release mentions "ACME-SUB becomes a
subsidiary" but the exact date requires inferring from the closing date
of the acquisition. The inference could be wrong by a few days.

**Why 0.75 authority?** Company press release — generally trustworthy
for factual statements about the company itself, but PR has a positive
spin bias.

### Example 3: Human-asserted fact

```scheme
(node n003
  :pred (instance-of @ACME-CORP organization)
  :conf 1.0 :auth 0.95 :perm public
  :src (asserted "admin"))
```

**Why 1.0 confidence?** The admin is directly asserting a categorical
fact they know to be true. There is no extraction uncertainty.

**Why 0.95 authority?** Admin user — trusted, but not 1.0 because humans
make mistakes. Reserve 1.0 for cryptographic or formal-verification
provenance (future `Derived` with Lean proofs).

### Example 4: Low-confidence extraction

```scheme
(node n004
  :pred (ceo-of @FOUNDER-1 @ACME-CORP)
  :valid (window "2018-06-01T00:00:00Z" "2024-12-31T00:00:00Z")
  :conf 0.55 :auth 0.60 :perm internal
  :src (extracted "news-article-blog" [200 450]
         (model "claude-sonnet-4" "2025-01")))
```

**Why 0.55 confidence?** A blog post mentions the CEO started "around
mid-2018" — the date is approximate, and the source is not authoritative.

**Why 0.60 authority?** Personal blog — low trust. The information may
be accurate, but the source doesn't warrant higher authority.

### Example 5: Formally derived fact

```scheme
(node n005
  :pred (subsidiary-of @ACME-SUB @ACME-CORP :since #date(2001-03-15))
  :conf 1.0 :auth 0.90 :perm public
  :src (derived n002 "rule-subsidiary-confirm")
  :deps [n002])
```

**Why 1.0 confidence?** Derived from an existing node via a formal rule.
If the rule is sound and the source node is correct, the derivation is
guaranteed correct. The confidence is inherited from the derivation
logic, not the source.

**Why 0.90 authority?** Inherits from the source node's authority. The
derivation rule itself doesn't add or remove trust in the source.

## Common Mistakes

### Mistake 1: Always 1.0 confidence

```scheme
; Bad — LLM always sets conf to 1.0
:conf 1.0
```

**Fix**: The system now applies **provenance-based default confidence**
automatically when `:conf` is omitted. These defaults are **policy constants**
informed by academic literature and KG trust tier conventions — the literature
supports the *ordering* (verbatim > extracted > summary > asserted) but not
the exact point values. Values will be revised when empirical data becomes
available (see M3: Empirical Reliability Table in `confidence-calibration-research.md`).

| Provenance | Default `:conf` | Allowed agent range | Exceeding range requires |
|------------|----------------|---------------------|-------------------------|
| `Verbatim` | 0.95 | [0.90, 0.98] | Exceeding rejected/logged |
| `Extracted` | 0.80 | [0.60, 0.90] | >0.90 needs corroboration |
| `Summary` | 0.75 | [0.50, 0.85] | >0.85 needs corroboration |
| `Asserted` | 0.60 | [0.30, 0.80] | >0.80 needs verified principal |
| `Derived` | min(deps) × 0.95 | N/A (computed) | See research doc §4.5 |

**Band clipping**: Agent self-assessment within the allowed range is accepted
(the relative signal is valuable). Values exceeding the range require
corroboration evidence (same canonical predicate from ≥2 independent
`(principal, model)` pairs) or are clamped with a warning. Values below the
range suggest the knowledge may not be worth inserting.

**Derived decay**: `conf_derived = min(active_deps' conf) × rule_reliability`.
Rule reliability defaults to 0.95 (rules can be misapplied); formally
verified rules (future Lean proofs) use 1.0 (no decay). This ensures longer
derivation chains accumulate uncertainty geometrically.

**Academic basis**: See `docs/confidence-calibration-research.md` for the
full literature review (7 papers, 2023–2025). Key findings:
- LLM verbalized confidence is systematically overconfident but carries signal
  (Tian et al. 2023) — band clipping preserves the signal while constraining
  the inflation.
- Inverse correlation between confidence and accuracy in clinical settings
  (r=−0.40, JMIR 2025) — fixed defaults based on evidence type are more
  reliable than agent self-assessment.
- KG trust tiers (Gold/Silver/Bronze) provide production-proven ordering.
- Future: Empirical Reliability Table (M3) will replace constants with
  measured accuracy per `(provenance × model)` pair.

Only use 1.0 for `Derived` with formal proofs (future work).

### Mistake 2: Confidence = Authority

```scheme
; Bad — using the same value for both
:conf 0.85 :auth 0.85
```

**Fix**: They measure different things. A high-authority source (Reuters,
0.85) reporting an uncertain rumor should have high authority (0.85) but
lower confidence (0.50). A low-authority source (blog, 0.30) stating a
verifiable numeric fact should have low authority (0.30) but high
confidence (0.90).

### Mistake 3: Confidence too granular

```scheme
; Bad — false precision
:conf 0.873
```

**Fix**: Use at most 2 decimal places. Confidence is a subjective
measure — the difference between 0.87 and 0.88 is meaningless. Use
0.85, 0.90, 0.95 — increments of 0.05 are sufficient.

### Mistake 4: Not adjusting for extraction difficulty

```scheme
; Bad — high confidence for hard extraction
:pred (ceo-of @X @Y :since #date(2018-06-01))
:conf 0.95  ; but the date was inferred from "around mid-2018"
```

**Fix**: If the extraction required inference (date normalization,
entity disambiguation, multi-hop reasoning), reduce confidence by
0.10–0.20 compared to a direct extraction.

## Query-Time Calibration

When querying, use `min_conf` to filter results:

```json
{
  "pattern": ["revenue", "?org", "?amount"],
  "options": {
    "min_conf": 0.70
  }
}
```

This filters out low-confidence nodes before arbitration. Typical
thresholds:

| Use case | `min_conf` | Rationale |
|----------|-----------|-----------|
| Financial analysis | 0.80 | Cannot afford wrong numbers |
| Due diligence | 0.90 | Need high-confidence facts only |
| Exploratory research | 0.50 | Cast a wide net |
| Background context | 0.30 | Accept weak signals for context |

**Note**: With provenance-based defaults, `Asserted` nodes default to
`conf=0.60`. Using `min_conf: 0.70` will silently exclude all `Asserted`
nodes (including human assertions). If you need human-asserted facts in
results, either lower the threshold or have the asserter provide an
explicit `:conf` above the threshold.

## Conflict Arbitration Interaction

When multiple nodes match the same query, the `ConflictPolicy` uses
authority (not confidence) to resolve:

- **LatestWins**: Highest authority wins; tiebreak by most recent
  validity start. If both are tied, marks `Ambiguous`.
- **HighestAuthority**: Strictly by authority. Ties mark `Ambiguous`.
- **Unanimous**: Only returns if all sources agree on the predicate.

Confidence is **not** used in arbitration — it's a pre-filter via
`min_conf`. This design prevents a high-confidence-but-wrong node from
overriding a lower-confidence-but-correct node from a more authoritative
source.

## Version History

- v0.3 (2026-09-15): Post-review revision. Added band clipping table with
  allowed agent ranges. Added Derived multiplicative decay formula. Added
  min_conf interaction note for Asserted nodes. Framed values as "policy
  constants" not "scientific conclusions." See `confidence-calibration-research.md`
  v0.3 for full revision details.
- v0.2 (2026-09-15): Added provenance-based default confidence table with
  academic citations. See `docs/confidence-calibration-research.md` for
  full literature review (7 papers, 2023–2025).
- v0.1 (2026-09-14): Initial calibration guide for v0.1.0 release.
