//! SemEquiv — structured semantic equivalence comparison.
//!
//! Per `design-rationale.md` §5 "Why SemEquiv Instead of Vector Cosine?":
//! Cosine similarity is a continuous measure that can be gamed: two completely
//! different facts can have high cosine similarity if they use similar
//! vocabulary. SemEquiv uses structured comparison: exact match, type match,
//! and predicate overlap. This produces auditable results.
//!
//! ## Calibrated Scoring (v2)
//!
//! The scoring philosophy is aligned with Factum's core tenet: every entity,
//! every literal value must be traceable. A decoded node with the wrong entity
//! or wrong number is **semantically wrong**, not "structurally similar".
//!
//! | Level | Score | Condition |
//! |-------|-------|-----------|
//! | Exact | 1.0 | All fields match except `id` and `note` |
//! | MetadataDrift | 0.85 | Predicate (head + args + values) fully match,
//! |       |       | but confidence/authority/provenance/validity differs |
//! | WrongValues | ≤0.3 | Head + arg count + arg types match, but entity IDs
//! |       |       | or literal values differ. Score is 0.3 max, lowered
//! |       |       | further if multiple values are wrong. |
//! | Different | 0.0 | Head differs, or arg count mismatches, or term types
//! |       |       | are incompatible |
//!
//! Key changes from v1:
//! - `Entailed` removed (was never implemented, just a name).
//! - `StructurallySimilar` renamed to `WrongValues` with max score 0.3.
//! - f32 comparisons use epsilon (1e-6) instead of exact equality.
//! - Entity identity and literal equality dominate the score.

use factum_core::types::*;
use factum_core::morphemes::{MorphemeRegistry, MorphemeKind};
use std::sync::Arc;

/// Epsilon for f32 confidence/authority comparison.
const F32_EPS: f32 = 1e-6;

/// Equivalence level — ordinal ranking of semantic closeness.
#[derive(Clone, Debug, PartialEq)]
pub enum EquivLevel {
    /// All fields match except `id` and `note`. Score = 1.0.
    Exact,
    /// Predicate fully matches (head + args + values), but metadata
    /// (confidence, authority, provenance, validity, permissions, deps,
    /// status) differs. Score = 0.85.
    MetadataDrift,
    /// Head matches, arg count + types match, but entity IDs or literal
    /// values differ. Max score = 0.3. Lowered further if multiple values
    /// are wrong.
    WrongValues,
    /// Head differs, arg count mismatches, or term types are incompatible.
    /// Score = 0.0.
    Different,
}

/// Equivalence comparison result.
#[derive(Clone, Debug)]
pub struct EquivResult {
    pub level: EquivLevel,
    /// Score in [0, 1]. Used for computing semantic round-trip rate.
    pub score: f32,
    /// Human-readable differences (auditable trail).
    pub differences: Vec<String>,
}

impl EquivResult {
    fn exact() -> Self {
        Self {
            level: EquivLevel::Exact,
            score: 1.0,
            differences: Vec::new(),
        }
    }

    fn metadata_drift(differences: Vec<String>) -> Self {
        Self {
            level: EquivLevel::MetadataDrift,
            score: 0.85,
            differences,
        }
    }

    fn wrong_values(score: f32, differences: Vec<String>) -> Self {
        Self {
            level: EquivLevel::WrongValues,
            score: score.min(0.3),
            differences,
        }
    }

    fn different(reason: impl Into<String>) -> Self {
        Self {
            level: EquivLevel::Different,
            score: 0.0,
            differences: vec![reason.into()],
        }
    }
}

/// SemEquiv — structured semantic equivalence comparator.
///
/// Compares two `Node`s (or `Predicate`s, `Term`s) and produces an
/// `EquivResult` with a level, score, and auditable difference list.
pub struct SemEquiv {
    registry: Arc<MorphemeRegistry>,
}

impl SemEquiv {
    /// Create a new SemEquiv with the given morpheme registry.
    pub fn new(registry: Arc<MorphemeRegistry>) -> Self {
        Self { registry }
    }

    /// Compare two nodes for semantic equivalence.
    ///
    /// The `id` and `note` fields are intentionally excluded — two nodes
    /// with different IDs but identical content are semantically equivalent.
    pub fn compare(&self, original: &Node, decoded: &Node) -> EquivResult {
        let mut diffs = Vec::new();

        // Compare predicates first — this is the core content.
        let pred_result = self.compare_predicates(&original.predicate, &decoded.predicate);
        match pred_result.level {
            EquivLevel::Different => return pred_result,
            EquivLevel::WrongValues => {
                diffs.extend(pred_result.differences);
                // If predicate values are wrong, the overall result can't be
                // better than WrongValues. Still collect metadata diffs for
                // the full audit trail.
                self.collect_metadata_diffs(original, decoded, &mut diffs);
                return EquivResult::wrong_values(pred_result.score, diffs);
            }
            EquivLevel::MetadataDrift => {
                diffs.extend(pred_result.differences);
            }
            EquivLevel::Exact => {}
        }

        // Predicates match exactly — now check metadata.
        self.collect_metadata_diffs(original, decoded, &mut diffs);

        if diffs.is_empty() {
            EquivResult::exact()
        } else {
            EquivResult::metadata_drift(diffs)
        }
    }

    /// Compare two predicates.
    pub fn compare_predicates(&self, a: &Predicate, b: &Predicate) -> EquivResult {
        let mut diffs = Vec::new();

        // Compare heads.
        let head_match = self.compare_heads(&a.head, &b.head);
        match head_match {
            HeadMatch::Same => {}
            HeadMatch::SameKind(kind_a, kind_b) => {
                diffs.push(format!(
                    "head kind match but different morpheme: {:?} vs {:?}",
                    kind_a, kind_b
                ));
                // Different head morpheme but same kind is still Different —
                // the assertion is about a completely different relation.
                return EquivResult::different(diffs.join("; "));
            }
            HeadMatch::Different => {
                return EquivResult::different(format!(
                    "predicate head kind mismatch: {:?} vs {:?}",
                    a.head, b.head
                ));
            }
        }

        // Compare arg count — mismatch is Different, not WrongValues.
        if a.args.len() != b.args.len() {
            return EquivResult::different(format!(
                "arg count mismatch: {} vs {}",
                a.args.len(),
                b.args.len()
            ));
        }

        // Compare each arg. Track how many have wrong values.
        let mut wrong_count = 0u32;

        for (i, (ta, tb)) in a.args.iter().zip(b.args.iter()).enumerate() {
            let term_result = self.compare_terms(ta, tb);
            match term_result.level {
                EquivLevel::Exact => {}
                EquivLevel::WrongValues => {
                    diffs.push(format!("arg[{}] value wrong: {}", i, term_result.differences.join("; ")));
                    wrong_count += 1;
                }
                EquivLevel::Different => {
                    return EquivResult::different(format!(
                        "arg[{}] type mismatch: {}",
                        i, term_result.differences.join("; ")
                    ));
                }
                EquivLevel::MetadataDrift => {
                    // Shouldn't happen for terms, but handle gracefully.
                    diffs.push(format!("arg[{}] drift: {}", i, term_result.differences.join("; ")));
                }
            }
        }

        // Compare named args.
        if a.named.len() != b.named.len() {
            return EquivResult::different(format!(
                "named arg count mismatch: {} vs {}",
                a.named.len(),
                b.named.len()
            ));
        }

        for (i, ((ka, va), (kb, vb))) in a.named.iter().zip(b.named.iter()).enumerate() {
            if ka != kb {
                return EquivResult::different(format!(
                    "named[{}] key mismatch: {} vs {}",
                    i, ka, kb
                ));
            }
            let term_result = self.compare_terms(va, vb);
            match term_result.level {
                EquivLevel::Exact => {}
                EquivLevel::WrongValues => {
                    diffs.push(format!("named[{}] value wrong: {}", i, term_result.differences.join("; ")));
                    wrong_count += 1;
                }
                EquivLevel::Different => {
                    return EquivResult::different(format!(
                        "named[{}] value type mismatch: {}",
                        i, term_result.differences.join("; ")
                    ));
                }
                EquivLevel::MetadataDrift => {
                    diffs.push(format!("named[{}] drift: {}", i, term_result.differences.join("; ")));
                }
            }
        }

        if wrong_count > 0 {
            // Score: start at 0.3, subtract 0.1 per wrong value (min 0.0).
            // With 1 wrong value out of 2: score = 0.3 - 0.1 = 0.2
            // With all wrong: score = 0.3 - 0.1*total → clamped to 0.0
            let penalty = 0.1 * wrong_count as f32;
            let score = (0.3 - penalty).max(0.0);
            EquivResult::wrong_values(score, diffs)
        } else if diffs.is_empty() {
            EquivResult::exact()
        } else {
            EquivResult::metadata_drift(diffs)
        }
    }

    /// Compare two terms.
    pub fn compare_terms(&self, a: &Term, b: &Term) -> EquivResult {
        match (a, b) {
            (Term::Var(sa), Term::Var(sb)) => {
                if sa == sb {
                    EquivResult::exact()
                } else {
                    // Variable name differs — this is WrongValues, not Different.
                    // The variable binds to something, and the binding identity matters.
                    EquivResult::wrong_values(0.2, vec![format!(
                        "var name: {} vs {}", sa, sb
                    )])
                }
            }
            (Term::Ent(ea), Term::Ent(eb)) => {
                if ea == eb {
                    EquivResult::exact()
                } else {
                    // Entity identity is paramount in Factum.
                    // Wrong entity = wrong assertion. Max score 0.3.
                    EquivResult::wrong_values(0.3, vec![format!(
                        "entity: {} vs {}", ea, eb
                    )])
                }
            }
            (Term::Lit(la), Term::Lit(lb)) => {
                if lit_eq(la, lb) {
                    EquivResult::exact()
                } else {
                    let same_type = matches!((la, lb),
                        (Literal::Dec(_, _), Literal::Dec(_, _)) |
                        (Literal::Str(_), Literal::Str(_)) |
                        (Literal::Date(_), Literal::Date(_)) |
                        (Literal::Dur(_), Literal::Dur(_)) |
                        (Literal::Bool(_), Literal::Bool(_)) |
                        (Literal::Uri(_), Literal::Uri(_))
                    );
                    if same_type {
                        EquivResult::wrong_values(0.3, vec![format!(
                            "literal value: {} vs {}",
                            la.to_canonical_string(),
                            lb.to_canonical_string()
                        )])
                    } else {
                        EquivResult::different(format!(
                            "literal type: {} vs {}",
                            la.to_canonical_string(),
                            lb.to_canonical_string()
                        ))
                    }
                }
            }
            (Term::Compound(pa), Term::Compound(pb)) => {
                self.compare_predicates(pa, pb)
            }
            (Term::List(items_a), Term::List(items_b)) => {
                if items_a.len() != items_b.len() {
                    return EquivResult::different(format!(
                        "list length: {} vs {}",
                        items_a.len(),
                        items_b.len()
                    ));
                }
                let mut diffs = Vec::new();
                let mut wrong_count = 0u32;
                for (i, (ta, tb)) in items_a.iter().zip(items_b.iter()).enumerate() {
                    let r = self.compare_terms(ta, tb);
                    match r.level {
                        EquivLevel::Exact => {}
                        EquivLevel::WrongValues => {
                            wrong_count += 1;
                            diffs.push(format!("list[{}]: {}", i, r.differences.join("; ")));
                        }
                        EquivLevel::Different => {
                            return EquivResult::different(format!(
                                "list[{}] type mismatch: {}", i, r.differences.join("; ")
                            ));
                        }
                        EquivLevel::MetadataDrift => {
                            diffs.push(format!("list[{}] drift: {}", i, r.differences.join("; ")));
                        }
                    }
                }
                if wrong_count > 0 {
                    let penalty = 0.1 * wrong_count as f32;
                    let score = (0.3 - penalty).max(0.0);
                    EquivResult::wrong_values(score, diffs)
                } else if diffs.is_empty() {
                    EquivResult::exact()
                } else {
                    EquivResult::metadata_drift(diffs)
                }
            }
            // Type mismatch — different term kinds.
            _ => EquivResult::different(format!(
                "term type mismatch: {} vs {}",
                term_kind_name(a),
                term_kind_name(b)
            )),
        }
    }

    /// Compare two node sequences and return the average score.
    pub fn compare_sequence(&self, original: &[Node], decoded: &[Node]) -> f32 {
        if original.is_empty() && decoded.is_empty() {
            return 1.0;
        }
        if original.len() != decoded.len() {
            return 0.0;
        }

        let total: f32 = original
            .iter()
            .zip(decoded.iter())
            .map(|(o, d)| self.compare(o, d).score)
            .sum();
        total / original.len() as f32
    }

    // ─── Internal helpers ─────────────────────────────────────

    fn collect_metadata_diffs(&self, a: &Node, b: &Node, diffs: &mut Vec<String>) {
        if a.validity != b.validity {
            diffs.push("validity mismatch".to_string());
        }
        if a.provenance != b.provenance {
            diffs.push("provenance mismatch".to_string());
        }
        if (a.confidence.0 - b.confidence.0).abs() > F32_EPS {
            diffs.push(format!(
                "confidence: {} vs {}",
                a.confidence.0, b.confidence.0
            ));
        }
        if (a.authority.0 - b.authority.0).abs() > F32_EPS {
            diffs.push(format!(
                "authority: {} vs {}",
                a.authority.0, b.authority.0
            ));
        }
        if a.permissions != b.permissions {
            diffs.push("permissions mismatch".to_string());
        }
        if a.deps != b.deps {
            diffs.push("deps mismatch".to_string());
        }
        if a.status != b.status {
            diffs.push(format!(
                "status mismatch: {:?} vs {:?}",
                a.status, b.status
            ));
        }
    }

    fn compare_heads(&self, a: &PredicateHead, b: &PredicateHead) -> HeadMatch {
        let (kind_a, name_a) = self.head_info(a);
        let (kind_b, name_b) = self.head_info(b);

        if name_a == name_b {
            return HeadMatch::Same;
        }

        if kind_a == kind_b {
            return HeadMatch::SameKind(kind_a, kind_b);
        }

        HeadMatch::Different
    }

    fn head_info(&self, head: &PredicateHead) -> (MorphemeKind, String) {
        match head {
            PredicateHead::Id(id) => {
                if let Some(def) = self.registry.lookup_id(*id) {
                    return (def.kind, def.name.to_string());
                }
                (MorphemeKind::Relation, format!("M{}", id.0))
            }
            PredicateHead::Name(name) => {
                if let Some(def) = self.registry.lookup(name) {
                    return (def.kind, def.name.to_string());
                }
                (infer_kind_from_name(name), name.to_string())
            }
        }
    }
}

/// Compare two literals with epsilon for Dec values.
fn lit_eq(a: &Literal, b: &Literal) -> bool {
    match (a, b) {
        (Literal::Dec(ma, sa), Literal::Dec(mb, sb)) => {
            // Exact comparison for Dec — these are fixed-point, not float.
            // Same mantissa and scale = same value.
            ma == mb && sa == sb
        }
        _ => a == b, // Str, Date, Dur, Bool, Uri use PartialEq
    }
}

enum HeadMatch {
    Same,
    SameKind(MorphemeKind, MorphemeKind),
    Different,
}

fn infer_kind_from_name(name: &str) -> MorphemeKind {
    if name.ends_with("-of") || name.ends_with("-by") || name.ends_with("-in") {
        return MorphemeKind::Relation;
    }
    if name.starts_with("is-") || name == "instance-of" {
        return MorphemeKind::Relation;
    }
    MorphemeKind::Relation
}

fn term_kind_name(t: &Term) -> &'static str {
    match t {
        Term::Var(_) => "Var",
        Term::Ent(_) => "Ent",
        Term::Lit(_) => "Lit",
        Term::Compound(_) => "Compound",
        Term::List(_) => "List",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use factum_core::morphemes::MorphemeRegistry;

    fn make_registry() -> Arc<MorphemeRegistry> {
        Arc::new(MorphemeRegistry::with_seeds())
    }

    fn make_node(id: &str, pred: Predicate) -> Node {
        Node::new(id, pred)
    }

    // ─── Exact match tests ────────────────────────────────────

    #[test]
    fn test_exact_match_same_id() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]);
        let a = make_node("n1", pred.clone());
        let b = make_node("n1", pred);
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::Exact);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn test_exact_match_different_id() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]);
        let a = make_node("n1", pred.clone());
        let b = make_node("n2", pred);
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::Exact);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn test_exact_match_different_note() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ]);
        let a = make_node("n1", pred.clone()).with_note("v1 report");
        let b = make_node("n2", pred).with_note("completely different note");
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::Exact);
        assert_eq!(result.score, 1.0);
    }

    // ─── MetadataDrift tests (predicate matches, metadata differs) ─

    #[test]
    fn test_metadata_drift_confidence() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]);
        let a = make_node("n1", pred.clone()).with_confidence(Confidence(0.85));
        let b = make_node("n2", pred).with_confidence(Confidence(0.70));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::MetadataDrift);
        assert_eq!(result.score, 0.85);
    }

    #[test]
    fn test_metadata_drift_authority() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]);
        let a = make_node("n1", pred.clone()).with_authority(Authority(0.9));
        let b = make_node("n2", pred).with_authority(Authority(0.5));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::MetadataDrift);
    }

    #[test]
    fn test_metadata_drift_provenance() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("5000000.00").unwrap()),
            ]);
        let a = make_node("n1", pred.clone()).with_provenance(Provenance::Asserted {
            by: Principal(smol_str::SmolStr::new("analyst-1")),
        });
        let b = make_node("n2", pred).with_provenance(Provenance::Asserted {
            by: Principal(smol_str::SmolStr::new("analyst-2")),
        });
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::MetadataDrift);
    }

    // ─── WrongValues tests (calibrated scoring) ───────────────

    #[test]
    fn test_wrong_entity_score_le_0_3() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = make_node("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));
        let b = make_node("n2", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("MOUNTAIN-VIEW")]));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::WrongValues);
        assert!(result.score <= 0.3, "wrong entity must score <= 0.3, got {}", result.score);
    }

    #[test]
    fn test_wrong_literal_score_le_0_3() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = make_node("n1", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ]));
        let b = make_node("n2", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("2000000.00").unwrap()),
            ]));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::WrongValues);
        assert!(result.score <= 0.3, "wrong literal must score <= 0.3, got {}", result.score);
    }

    #[test]
    fn test_all_values_wrong_scores_zero() {
        // Both entity and literal wrong → score should be very low or zero.
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = make_node("n1", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ]));
        let b = make_node("n2", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("NOVALENS"),
                Term::lit(Literal::dec_from_str("99.00").unwrap()),
            ]));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::WrongValues);
        // 2 wrong values: 0.3 - 0.1*2 = 0.1
        assert!((result.score - 0.1).abs() < 0.01, "expected 0.1, got {}", result.score);
    }

    #[test]
    fn test_wrong_var_name() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = make_node("n1", Predicate::new("shareholder-major")
            .with_args(vec![Term::ent("ACME-CORP"), Term::var("p")]));
        let b = make_node("n2", Predicate::new("shareholder-major")
            .with_args(vec![Term::ent("ACME-CORP"), Term::var("q")]));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::WrongValues);
        assert!(result.score <= 0.3);
    }

    // ─── Different tests ──────────────────────────────────────

    #[test]
    fn test_different_head_kind() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = make_node("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));
        let b = make_node("n2", Predicate::new("active")
            .with_args(vec![Term::ent("ACME-CORP")]));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::Different);
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn test_different_arg_count() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = make_node("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]));
        let b = make_node("n2", Predicate::new("located-in")
            .with_args(vec![Term::ent("X")]));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::Different);
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn test_different_term_types() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = make_node("n1", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000").unwrap()),
            ]));
        let b = make_node("n2", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::var("amount"),
            ]));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::Different);
    }

    // ─── Predicate comparison tests ────────────────────────────

    #[test]
    fn test_compare_predicates_exact() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]);
        let b = Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]);
        let result = sequiv.compare_predicates(&a, &b);
        assert_eq!(result.level, EquivLevel::Exact);
    }

    #[test]
    fn test_compare_predicates_named_args() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = Predicate::new("revenue")
            .with_args(vec![Term::ent("X"), Term::lit(Literal::dec_from_str("1000").unwrap())])
            .with_named("period", Term::lit(Literal::Date(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())));
        let b = Predicate::new("revenue")
            .with_args(vec![Term::ent("X"), Term::lit(Literal::dec_from_str("1000").unwrap())])
            .with_named("period", Term::lit(Literal::Date(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())));
        let result = sequiv.compare_predicates(&a, &b);
        assert_eq!(result.level, EquivLevel::Exact);
    }

    // ─── Term comparison tests ────────────────────────────────

    #[test]
    fn test_compare_terms_exact() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = Term::ent("ACME-CORP");
        let b = Term::ent("ACME-CORP");
        assert_eq!(sequiv.compare_terms(&a, &b).level, EquivLevel::Exact);
        let a = Term::lit(Literal::dec_from_str("42.00").unwrap());
        let b = Term::lit(Literal::dec_from_str("42.00").unwrap());
        assert_eq!(sequiv.compare_terms(&a, &b).level, EquivLevel::Exact);
    }

    #[test]
    fn test_compare_terms_different_kinds() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = Term::ent("ACME-CORP");
        let b = Term::var("x");
        assert_eq!(sequiv.compare_terms(&a, &b).level, EquivLevel::Different);
    }

    #[test]
    fn test_compare_terms_literal_type_mismatch() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let a = Term::lit(Literal::dec_from_str("42").unwrap());
        let b = Term::lit(Literal::Str(smol_str::SmolStr::new("42")));
        assert_eq!(sequiv.compare_terms(&a, &b).level, EquivLevel::Different);
    }

    // ─── Sequence comparison ──────────────────────────────────

    #[test]
    fn test_compare_sequence_all_exact() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let nodes_a: Vec<Node> = (0..5).map(|i| {
            make_node(format!("n{}", i).as_str(), Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")]))
        }).collect();
        let nodes_b: Vec<Node> = (0..5).map(|i| {
            make_node(format!("m{}", i).as_str(), Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")]))
        }).collect();
        let score = sequiv.compare_sequence(&nodes_a, &nodes_b);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_compare_sequence_mixed() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let nodes_a = vec![
            make_node("n1", Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")])),
            make_node("n2", Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Z")])),
        ];
        let nodes_b = vec![
            make_node("m1", Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")])),
            make_node("m2", Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("W")])),
        ];
        let score = sequiv.compare_sequence(&nodes_a, &nodes_b);
        // n1=m1 exact (1.0), n2≈m2 WrongValues (0.2)
        // avg = (1.0 + 0.2) / 2 = 0.6
        assert!((score - 0.6).abs() < 0.01, "expected 0.6, got {}", score);
    }

    #[test]
    fn test_compare_sequence_length_mismatch() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let nodes_a = vec![make_node("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]))];
        let nodes_b = vec![
            make_node("m1", Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")])),
            make_node("m2", Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")])),
        ];
        let score = sequiv.compare_sequence(&nodes_a, &nodes_b);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_compare_sequence_empty() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let score = sequiv.compare_sequence(&[], &[]);
        assert_eq!(score, 1.0);
    }

    // ─── f32 epsilon tests ────────────────────────────────────

    #[test]
    fn test_confidence_epsilon_not_drift() {
        // Tiny float difference should NOT trigger metadata drift.
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]);
        let a = make_node("n1", pred.clone()).with_confidence(Confidence(0.85));
        let b = make_node("n2", pred).with_confidence(Confidence(0.85 + 1e-7));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::Exact);
    }

    #[test]
    fn test_confidence_significant_drift() {
        let reg = make_registry();
        let sequiv = SemEquiv::new(reg);
        let pred = Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]);
        let a = make_node("n1", pred.clone()).with_confidence(Confidence(0.85));
        let b = make_node("n2", pred).with_confidence(Confidence(0.84));
        let result = sequiv.compare(&a, &b);
        assert_eq!(result.level, EquivLevel::MetadataDrift);
    }
}
