//! Verifier framework — formal validation of node correctness.
//!
//! ## Built-in Verifiers (planned)
//! - `ArithmeticVerifier`: Check literal arithmetic consistency
//! - `SolverVerifier`: Call Z3 for constraint satisfaction (z3.rs)
//! - `LeanVerifier`: Spawn Lean process to verify proof assertions
//! - `SchemaVerifier`: Morpheme signature type checking
//!
//! In v0.1, we implement `ArithmeticVerifier` and `SchemaVerifier`.
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
        reg.add(Box::new(ArithmeticVerifier));
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

        // Basic arity check (simplified — full type checking is future work)
        // Parse signature to count expected params
        let sig = &def.signature.raw;
        if sig.contains("->") {
            let params_part = sig.split("->").next().unwrap_or("");
            let param_count = params_part.split(',').filter(|s| !s.trim().is_empty()).count();

            if param_count > 0 && node.predicate.args.len() != param_count {
                return Verdict::Fail(format!(
                    "arity mismatch: expected {} args, got {}",
                    param_count, node.predicate.args.len()
                ));
            }
        }

        // Check that Entity morphemes aren't used as Relations
        if def.kind == MorphemeKind::Entity && !node.predicate.args.is_empty() {
            return Verdict::Fail("entity-type morpheme used with arguments".into());
        }

        Verdict::Pass
    }
}

/// Arithmetic verifier — checks numeric consistency.
///
/// In v0.1, this checks:
/// - Decimal values are within valid range
/// - No overflow in stored values
pub struct ArithmeticVerifier;

impl Verifier for ArithmeticVerifier {
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

        // shareholder-major expects 4 args: org, holder, since, stake
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
    fn test_arithmetic_verifier() {
        let verifier = ArithmeticVerifier;

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
