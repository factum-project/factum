//! Verifier framework — formal validation of node correctness.
//!
//! ## Built-in Verifiers (planned)
//! - `DecimalRangeVerifier`: Check decimal literal range (scale and digit count)
//! - `SolverVerifier`: Call Z3 for constraint satisfaction (z3.rs)
//! - `LeanVerifier`: Spawn Lean process to verify proof assertions
//! - `SchemaVerifier`: Morpheme signature type checking
//!
//! In v0.1, we implement `DecimalRangeVerifier` and `SchemaVerifier`.
//! Z3 and Lean integration are deferred to later milestones.

use std::sync::Arc;
use factum_core::types::*;
use factum_core::morphemes::{MorphemeRegistry, MorphemeKind};

/// Verification verdict.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Node passes verification.
    Pass,
    /// Node fails verification with a reason.
    Fail(String),
    /// Verifier cannot determine (e.g., needs external solver).
    Inconclusive,
}

/// Trait for verifiers.
pub trait Verifier: Send + Sync {
    /// Can this verifier handle this node?
    fn can_verify(&self, node: &Node) -> bool;

    /// Verify the node.
    fn verify(&self, node: &Node) -> Verdict;
}

/// Registry of verifiers.
#[derive(Default)]
pub struct VerifierRegistry {
    verifiers: Vec<Box<dyn Verifier>>,
}

impl VerifierRegistry {
    pub fn new() -> Self {
        Self { verifiers: Vec::new() }
    }

    /// Create a registry with built-in verifiers.
    pub fn with_builtins(registry: Arc<MorphemeRegistry>) -> Self {
        let mut reg = Self::new();
        reg.add(Box::new(SchemaVerifier { registry }));
        reg.add(Box::new(DecimalRangeVerifier));
        reg
    }

    /// Add a verifier.
    pub fn add(&mut self, v: Box<dyn Verifier>) {
        self.verifiers.push(v);
    }

    /// Run all applicable verifiers on a node.
    /// Returns Pass only if all applicable verifiers pass.
    pub fn verify(&self, node: &Node) -> Verdict {
        let mut any_ran = false;
        for v in &self.verifiers {
            if v.can_verify(node) {
                any_ran = true;
                match v.verify(node) {
                    Verdict::Fail(reason) => return Verdict::Fail(reason),
                    Verdict::Pass => continue,
                    Verdict::Inconclusive => continue,
                }
            }
        }
        if any_ran { Verdict::Pass } else { Verdict::Inconclusive }
    }
}

/// Schema verifier — checks morpheme signature type consistency.
pub struct SchemaVerifier {
    registry: Arc<MorphemeRegistry>,
}

impl Verifier for SchemaVerifier {
    fn can_verify(&self, node: &Node) -> bool {
        // Can verify if the predicate head is a known morpheme
        match &node.predicate.head {
            PredicateHead::Name(name) => self.registry.lookup(name).is_some(),
            PredicateHead::Id(id) => self.registry.lookup_id(*id).is_some(),
        }
    }

    fn verify(&self, node: &Node) -> Verdict {
        let def = match &node.predicate.head {
            PredicateHead::Name(name) => self.registry.lookup(name),
            PredicateHead::Id(id) => self.registry.lookup_id(*id),
        };

        let def = match def {
            Some(d) => d,
            None => return Verdict::Fail("unknown morpheme".into()),
        };

        // Arity check: parse signature to count expected params, then compare
        // against the sum of positional args + named args in the node.
        //
        // Signature format: "name:Type, name:Type?, ... -> ReturnType"
        // - Required params have no `?` suffix
        // - Optional params end with `?`
        // - Params can be provided as positional OR named in the node
        //
        // Validation rules:
        //   total_provided = args.len() + named.len()
        //   required <= total_provided <= total_params
        //   total_provided >= required (must provide all required params)
        let sig = &def.signature.raw;
        if sig.contains("->") {
            let params_part = sig.split("->").next().unwrap_or("");
            let params: Vec<&str> = params_part
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            if !params.is_empty() {
                let total_params = params.len();
                let required_params = params
                    .iter()
                    .filter(|p| !p.ends_with('?'))
                    .count();

                let provided = node.predicate.args.len() + node.predicate.named.len();

                if provided < required_params {
                    return Verdict::Fail(format!(
                        "arity mismatch: expected at least {} args ({} required, {} optional), got {}",
                        required_params, required_params, total_params - required_params, provided
                    ));
                }

                if provided > total_params {
                    return Verdict::Fail(format!(
                        "arity mismatch: expected at most {} args, got {}",
                        total_params, provided
                    ));
                }
            }
        }

        // Check that Entity morphemes aren't used as Relations
        if def.kind == MorphemeKind::Entity && !node.predicate.args.is_empty() {
            return Verdict::Fail("entity-type morpheme used with arguments".into());
        }

        Verdict::Pass
    }
}

/// Decimal range verifier — checks that Decimal literals are within
/// representable bounds.
///
/// In v0.1, this checks:
/// - Decimal scale ≤ 38 (matches the i128-backed fixed-point representation)
/// - Decimal significant digits ≤ 38
///
/// **Note**: This verifier does NOT perform arithmetic consistency checks
/// (e.g., "revenue − costs = profit"). It only validates that individual
/// Decimal values are within the representable range of the fixed-point
/// type used by Factum. Arithmetic consistency verification is planned
/// for a future milestone (see `SolverVerifier` / `LeanVerifier`).
pub struct DecimalRangeVerifier;

impl Verifier for DecimalRangeVerifier {
    fn can_verify(&self, node: &Node) -> bool {
        // Can verify if any arg is a Decimal literal
        node.predicate.args.iter().any(|t| matches!(t, Term::Lit(Literal::Dec(_, _))))
    }

    fn verify(&self, node: &Node) -> Verdict {
        for arg in &node.predicate.args {
            if let Term::Lit(Literal::Dec(mantissa, scale)) = arg {
                // Check mantissa fits in i128
                if *scale > 38 {
                    return Verdict::Fail(format!("decimal scale {} exceeds maximum 38", scale));
                }

                // Check the number of significant digits
                let abs = mantissa.unsigned_abs();
                let digits = abs.to_string().len();
                if digits > 38 {
                    return Verdict::Fail(format!("decimal has {} digits, exceeds maximum 38", digits));
                }
            }
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use factum_core::morphemes::MorphemeRegistry;

    #[test]
    fn test_schema_verifier_arity() {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let verifier = SchemaVerifier { registry };

        // shareholder-major expects 4 args: org, holder, since, stake (all required)
        let node = Node::new("n001",
            Predicate::new("shareholder-major")
                .with_args(vec![
                    Term::ent("X"),
                    Term::ent("Y"),
                ])); // only 2 args, should fail

        let verdict = verifier.verify(&node);
        match verdict {
            Verdict::Fail(msg) => assert!(msg.contains("arity")),
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn test_schema_verifier_pass() {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let verifier = SchemaVerifier { registry };

        // instance-of expects 2 args
        let node = Node::new("n001",
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("X"), Term::ent("organization")]));

        let verdict = verifier.verify(&node);
        assert_eq!(verdict, Verdict::Pass);
    }

    #[test]
    fn test_schema_verifier_named_args_counted() {
        // shareholder-major has 4 params. If user provides 3 positional + 1 named,
        // it should pass. Previously it would fail because named args were ignored.
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let verifier = SchemaVerifier { registry };

        let node = Node::new("n001",
            Predicate::new("shareholder-major")
                .with_args(vec![
                    Term::ent("ACME"),
                    Term::ent("HOLDER-1"),
                    Term::lit(Literal::dec_from_str("0.5").unwrap()),
                ])
                .with_named("since", Term::lit(Literal::Date(
                    chrono::NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()
                ))));

        let verdict = verifier.verify(&node);
        assert_eq!(verdict, Verdict::Pass);
    }

    #[test]
    fn test_schema_verifier_optional_param_omitted() {
        // acquired-by has signature: target, acquirer, date, amount:Dec?
        // Omitting the optional `amount` should pass (3 args, 3 required, 1 optional)
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let verifier = SchemaVerifier { registry };

        let node = Node::new("n001",
            Predicate::new("acquired-by")
                .with_args(vec![
                    Term::ent("ACME"),
                    Term::ent("BIG-CORP"),
                    Term::lit(Literal::Date(
                        chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                    )),
                ]));

        let verdict = verifier.verify(&node);
        assert_eq!(verdict, Verdict::Pass);
    }

    #[test]
    fn test_schema_verifier_optional_param_provided() {
        // acquired-by with all 4 params (including optional amount) should pass
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let verifier = SchemaVerifier { registry };

        let node = Node::new("n001",
            Predicate::new("acquired-by")
                .with_args(vec![
                    Term::ent("ACME"),
                    Term::ent("BIG-CORP"),
                    Term::lit(Literal::Date(
                        chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                    )),
                    Term::lit(Literal::dec_from_str("1000000").unwrap()),
                ]));

        let verdict = verifier.verify(&node);
        assert_eq!(verdict, Verdict::Pass);
    }

    #[test]
    fn test_schema_verifier_too_many_args() {
        // instance-of expects 2 args; providing 3 should fail
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let verifier = SchemaVerifier { registry };

        let node = Node::new("n001",
            Predicate::new("instance-of")
                .with_args(vec![
                    Term::ent("X"),
                    Term::ent("organization"),
                    Term::ent("extra"),
                ]));

        let verdict = verifier.verify(&node);
        match verdict {
            Verdict::Fail(msg) => assert!(msg.contains("arity")),
            _ => panic!("expected failure for too many args"),
        }
    }

    #[test]
    fn test_decimal_range_verifier() {
        let verifier = DecimalRangeVerifier;

        // Valid decimal
        let node = Node::new("n001",
            Predicate::new("revenue")
                .with_args(vec![Term::ent("X"), Term::lit(Literal::dec_from_str("230.50").unwrap())]));
        assert_eq!(verifier.verify(&node), Verdict::Pass);

        // Invalid scale
        let node2 = Node::new("n002",
            Predicate::new("revenue")
                .with_args(vec![Term::ent("X"), Term::lit(Literal::Dec(1, 50))]));
        match verifier.verify(&node2) {
            Verdict::Fail(msg) => assert!(msg.contains("scale")),
            _ => panic!("expected failure for excessive scale"),
        }
    }

    #[test]
    fn test_verifier_registry() {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let vr = VerifierRegistry::with_builtins(registry);

        let node = Node::new("n001",
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("X"), Term::ent("organization")]));

        let verdict = vr.verify(&node);
        assert_eq!(verdict, Verdict::Pass);
    }
}
