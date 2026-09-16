//! Confidence calibration policy.
//!
//! These values are **policy constants** informed by academic literature
//! and KG trust tier conventions, NOT scientific measurements. The
//! literature supports the *ordering* (verbatim > extracted > summary >
//! asserted) but not the *point values*. Values will be revised when
//! empirical data becomes available (see M3: Empirical Reliability Table
//! in `docs/confidence-calibration-research.md` §4.6).
//!
//! ## Key design decisions
//!
//! 1. **Policy constants, not science**: The specific numbers (0.95, 0.80,
//!    0.75, 0.60) are based on KG trust tier conventions (Gold/Silver/
//!    Bronze). No paper measured "the correct default confidence for
//!    LLM-extracted knowledge." The ordering is literature-supported;
//!    the point values are policy choices.
//! 2. **Band clipping**: Each provenance type has an allowed range for
//!    agent self-assessment. Within the band, agent signal is preserved
//!    (Tian et al. 2023: verbalized confidence is inflated but carries
//!    signal). Exceeding the band requires corroboration evidence.
//! 3. **Derived multiplicative decay**: `conf_derived = min(deps conf) ×
//!    rule_reliability`. Geometric decay ensures longer derivation chains
//!    accumulate uncertainty. Verified rules (future Lean proofs) use
//!    1.0 (no decay).
//! 4. **Empirical reliability table (M3)**: Future upgrade to replace
//!    constants with measured `(provenance × model × principal)` accuracy data.
//!    The `principal` dimension enables per-agent reliability tracking in
//!    multi-agent scenarios — same model, different agent instances may
//!    exhibit different extraction accuracy.

use crate::types::{Confidence, Provenance};

/// Default rule reliability for unverified derivation rules.
///
/// Each derivation step multiplies confidence by this factor, modeling
/// that rules can be misapplied even when premises are correct.
/// Formally verified rules (Lean proofs, future work) will use 1.0
/// (no decay).
pub const DEFAULT_RULE_RELIABILITY: f32 = 0.95;

/// Cold-start threshold for empirical reliability table (M3).
///
/// Below this many data points for a `(provenance, model, principal)` triple,
/// the policy constants are used instead of observed accuracy.
/// Prevents small-sample bias in the reliability estimate.
/// When principal is unknown or "system", falls back to `(provenance, model)` pair.
pub const EMPIRICAL_MIN_SAMPLES: usize = 10;

/// Provenance-based default confidence (policy constants).
///
/// When the caller does not provide an explicit `:conf` value, the system
/// assigns a default based on the evidence type (provenance variant).
///
/// **These are policy constants, not scientific measurements.**
/// The literature supports the ordering (verbatim > extracted > summary >
/// asserted) but not the specific point values. See
/// `docs/confidence-calibration-research.md` §4.1-4.2 for rationale.
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
/// Formula: `conf_derived = min(active_deps' conf) × rule_reliability`
///
/// - Uses `min()` because a derivation chain is only as strong as its
///   weakest link. If node A (conf=0.95) and node B (conf=0.40) jointly
///   derive C, C's confidence is limited by B's uncertainty.
/// - `rule_reliability` defaults to 0.95 (`DEFAULT_RULE_RELIABILITY`),
///   modeling that rules can be misapplied. Formally verified rules
///   (future Lean verifier) use 1.0 (no decay).
/// - If `dep_confidences` is empty, returns `Confidence(0.0)` — derivation
///   from no premises is worthless. (Cascade retraction should have
///   already handled retracted deps, but this is a safety net.)
///
/// # Decay examples
///
/// - 1 step: 0.80 × 0.95 = 0.76
/// - 5 steps: 0.80 × 0.95⁵ = 0.615
/// - 10 steps: 0.80 × 0.95¹⁰ = 0.476
pub fn derived_confidence(
    dep_confidences: &[Confidence],
    rule_reliability: f32,
) -> Confidence {
    if dep_confidences.is_empty() {
        return Confidence(0.0);
    }
    let min_conf = dep_confidences
        .iter()
        .map(|c| c.0)
        .fold(f32::INFINITY, f32::min);
    Confidence(min_conf * rule_reliability)
}

/// Convenience: `derived_confidence` with default rule reliability.
pub fn derived_confidence_default(dep_confidences: &[Confidence]) -> Confidence {
    derived_confidence(dep_confidences, DEFAULT_RULE_RELIABILITY)
}

/// Allowed confidence range for band clipping.
///
/// Returns `(lower, upper)` bounds for agent self-assessment.
///
/// - Values **within** the range: accepted (agent's relative judgment
///   is preserved — Tian et al. 2023 shows verbalized confidence carries
///   signal despite systematic inflation).
/// - Values **above** the range: require corroboration (same canonical
///   predicate from >= 2 independent `(principal, model)` pairs) or
///   are clamped with a warning. (Enforcement: M2+)
/// - Values **below** the range: suggest the knowledge may not be worth
///   inserting. System suggests reconsidering.
///
/// See `docs/confidence-calibration-research.md` §4.3 for full rationale.
pub fn confidence_band(p: &Provenance) -> (f32, f32) {
    match p {
        Provenance::Verbatim { .. } => (0.90, 0.98),
        Provenance::Extracted { .. } => (0.60, 0.90),
        Provenance::Summary { .. } => (0.50, 0.85),
        Provenance::Asserted { .. } => (0.30, 0.80),
        // Derived confidence is computed, not self-assessed.
        // No band applies.
        Provenance::Derived { .. } => (0.0, 1.0),
    }
}

/// Check if a confidence value is within the allowed band for a provenance.
///
/// Returns `Ok(())` if within band, or `Err(message)` describing the
/// violation (above or below). Used by `factum_assert` to emit warnings.
pub fn check_confidence_band(
    p: &Provenance,
    conf: Confidence,
) -> Result<(), String> {
    let (lower, upper) = confidence_band(p);
    if conf.0 > upper {
        Err(format!(
            "confidence {:.2} exceeds {} band upper bound {:.2}; \
             corroboration evidence required for values above {:.2}",
            conf.0,
            provenance_name(p),
            upper,
            upper
        ))
    } else if conf.0 < lower {
        Err(format!(
            "confidence {:.2} is below {} band lower bound {:.2}; \
             consider not inserting this knowledge",
            conf.0,
            provenance_name(p),
            lower
        ))
    } else {
        Ok(())
    }
}

/// Human-readable name for a provenance variant (for error messages).
fn provenance_name(p: &Provenance) -> &'static str {
    match p {
        Provenance::Verbatim { .. } => "Verbatim",
        Provenance::Extracted { .. } => "Extracted",
        Provenance::Summary { .. } => "Summary",
        Provenance::Asserted { .. } => "Asserted",
        Provenance::Derived { .. } => "Derived",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use smol_str::SmolStr;

    fn provenance_verbatim() -> Provenance {
        Provenance::Verbatim {
            doc: DocId::new("test-doc"),
            span: Span { start: 0, end: 100 },
        }
    }

    fn provenance_extracted() -> Provenance {
        Provenance::Extracted {
            doc: DocId::new("test-doc"),
            span: Span { start: 0, end: 100 },
            model: ModelRef {
                name: SmolStr::new("claude-sonnet-4"),
                version: SmolStr::new("2025-01"),
            },
        }
    }

    fn provenance_summary() -> Provenance {
        Provenance::Summary {
            doc: DocId::new("test-doc"),
            span: Span { start: 0, end: 200 },
        }
    }

    fn provenance_asserted() -> Provenance {
        Provenance::Asserted {
            by: Principal(SmolStr::new("user-a")),
        }
    }

    fn provenance_derived() -> Provenance {
        Provenance::Derived {
            from: NodeId::new("n001"),
            rule: RuleId(SmolStr::new("rule-001")),
        }
    }

    #[test]
    fn test_default_confidence_for_each_provenance() {
        // Table-driven test verifying the policy constant for each variant.
        assert_eq!(
            default_confidence_for_provenance(&provenance_verbatim()),
            Confidence(0.95)
        );
        assert_eq!(
            default_confidence_for_provenance(&provenance_extracted()),
            Confidence(0.80)
        );
        assert_eq!(
            default_confidence_for_provenance(&provenance_summary()),
            Confidence(0.75)
        );
        assert_eq!(
            default_confidence_for_provenance(&provenance_asserted()),
            Confidence(0.60)
        );
        // Derived without store access: fallback to 0.80 (conservative).
        assert_eq!(
            default_confidence_for_provenance(&provenance_derived()),
            Confidence(0.80)
        );
    }

    #[test]
    fn test_default_confidence_ordering() {
        // The literature-supported ordering: verbatim > extracted > summary > asserted.
        // Derived is a computed value, not part of the ordering.
        let v = default_confidence_for_provenance(&provenance_verbatim()).0;
        let e = default_confidence_for_provenance(&provenance_extracted()).0;
        let s = default_confidence_for_provenance(&provenance_summary()).0;
        let a = default_confidence_for_provenance(&provenance_asserted()).0;

        assert!(v > e, "Verbatim must be higher than Extracted");
        assert!(e > s, "Extracted must be higher than Summary");
        assert!(s > a, "Summary must be higher than Asserted");
    }

    #[test]
    fn test_derived_confidence_single_dep() {
        // Single dependency: conf × rule_reliability
        let deps = vec![Confidence(0.80)];
        let result = derived_confidence_default(&deps);
        assert!((result.0 - 0.76).abs() < 0.001, "expected 0.76, got {}", result.0);
    }

    #[test]
    fn test_derived_confidence_multi_dep_uses_min() {
        // Multi-dep: min(conf) × rule_reliability (weakest link)
        let deps = vec![Confidence(0.95), Confidence(0.40), Confidence(0.70)];
        let result = derived_confidence_default(&deps);
        // min(0.95, 0.40, 0.70) = 0.40; 0.40 × 0.95 = 0.38
        assert!((result.0 - 0.38).abs() < 0.001, "expected 0.38, got {}", result.0);
    }

    #[test]
    fn test_derived_confidence_empty_deps() {
        // No deps: derivation from no premises is worthless
        let result = derived_confidence_default(&[]);
        assert_eq!(result, Confidence(0.0));
    }

    #[test]
    fn test_derived_confidence_verified_rule() {
        // Formally verified rule: rule_reliability = 1.0 (no decay)
        let deps = vec![Confidence(0.80)];
        let result = derived_confidence(&deps, 1.0);
        assert_eq!(result, Confidence(0.80));
    }

    #[test]
    fn test_derived_confidence_chain_decay() {
        // Simulate a 5-step derivation chain: 0.80 × 0.95^5 = 0.615
        let mut conf = Confidence(0.80);
        for _ in 0..5 {
            conf = derived_confidence_default(&[conf]);
        }
        assert!(
            (conf.0 - 0.615).abs() < 0.01,
            "5-step chain should decay to ~0.615, got {}",
            conf.0
        );
    }

    #[test]
    fn test_confidence_band_ranges() {
        // Verify band bounds for each provenance type.
        let (lo, hi) = confidence_band(&provenance_verbatim());
        assert_eq!(lo, 0.90);
        assert_eq!(hi, 0.98);

        let (lo, hi) = confidence_band(&provenance_extracted());
        assert_eq!(lo, 0.60);
        assert_eq!(hi, 0.90);

        let (lo, hi) = confidence_band(&provenance_summary());
        assert_eq!(lo, 0.50);
        assert_eq!(hi, 0.85);

        let (lo, hi) = confidence_band(&provenance_asserted());
        assert_eq!(lo, 0.30);
        assert_eq!(hi, 0.80);

        // Derived: no band (computed, not self-assessed)
        let (lo, hi) = confidence_band(&provenance_derived());
        assert_eq!(lo, 0.0);
        assert_eq!(hi, 1.0);
    }

    #[test]
    fn test_check_confidence_band_in_range() {
        // Within band: Ok
        assert!(check_confidence_band(&provenance_extracted(), Confidence(0.75)).is_ok());
        assert!(check_confidence_band(&provenance_extracted(), Confidence(0.60)).is_ok());
        assert!(check_confidence_band(&provenance_extracted(), Confidence(0.90)).is_ok());
    }

    #[test]
    fn test_check_confidence_band_above() {
        // Above band: Err with corroboration message
        let result = check_confidence_band(&provenance_extracted(), Confidence(0.95));
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("exceeds"), "message should mention 'exceeds': {}", msg);
        assert!(msg.contains("corroboration"), "message should mention 'corroboration': {}", msg);
    }

    #[test]
    fn test_check_confidence_band_below() {
        // Below band: Err with "consider not inserting" message
        let result = check_confidence_band(&provenance_extracted(), Confidence(0.40));
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("below"), "message should mention 'below': {}", msg);
        assert!(msg.contains("consider not inserting"), "message should suggest not inserting: {}", msg);
    }

    #[test]
    fn test_check_confidence_band_asserted_high() {
        // Asserted at 0.85: above band [0.30, 0.80], needs verified principal
        let result = check_confidence_band(&provenance_asserted(), Confidence(0.85));
        assert!(result.is_err());
    }

    #[test]
    fn test_check_confidence_band_derived_always_ok() {
        // Derived band is [0.0, 1.0] — any value passes
        assert!(check_confidence_band(&provenance_derived(), Confidence(0.0)).is_ok());
        assert!(check_confidence_band(&provenance_derived(), Confidence(1.0)).is_ok());
        assert!(check_confidence_band(&provenance_derived(), Confidence(0.5)).is_ok());
    }
}
