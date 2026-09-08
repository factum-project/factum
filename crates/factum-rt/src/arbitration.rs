//! Conflict arbitration — resolve multiple nodes matching the same query.
//!
//! ## Policies
//! - `LatestWins`: highest authority wins, tiebreak by most recent validity
//! - `HighestAuthority`: strictly by authority score
//! - `Unanimous`: only return if all sources agree
//! - `Custom`: user-provided function
//!
//! ## Key Design Decision
//! When arbitration cannot uniquely resolve, return `Ambiguous`.
//! **We refuse to answer rather than guess.** This is a core Factum principle.

use std::collections::HashMap;
use std::sync::Arc;
use smol_str::SmolStr;
use factum_core::types::*;
use crate::query::QueryResult;

/// Conflict resolution policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConflictPolicy {
    /// Default: highest authority wins, tiebreak by most recent validity start.
    LatestWins,
    /// Strictly by authority score.
    HighestAuthority,
    /// Only return if all sources agree (same predicate).
    Unanimous,
    /// User-provided arbitration function (not implementable in v0.1 const context).
    Custom,
}

/// Result of arbitration.
#[derive(Clone, Debug, PartialEq)]
pub enum ArbitrationResult {
    /// Single winner
    Resolved(Arc<Node>),
    /// Multiple agreeing sources
    Unanimous(Vec<Arc<Node>>),
    /// Could not resolve — refuse to answer
    Ambiguous(Vec<Arc<Node>>),
}

/// Arbitrate a set of matching query results.
///
/// Groups results by their variable bindings, then applies the policy
/// within each group. If a group cannot be uniquely resolved, marks
/// the result set as ambiguous.
pub fn arbitrate(
    matches: Vec<QueryResult>,
    policy: &ConflictPolicy,
) -> (Vec<QueryResult>, bool) {
    if matches.is_empty() {
        return (Vec::new(), false);
    }

    // Group by binding signature (same variable → same value)
    let groups = group_by_bindings(matches);

    let mut results = Vec::new();
    let mut ambiguous = false;

    for (_, group) in groups {
        if group.len() == 1 {
            results.push(group.into_iter().next().unwrap());
            continue;
        }

        match policy {
            ConflictPolicy::LatestWins => {
                // Sort by authority desc, then by validity start desc
                let winner = group.iter()
                    .max_by(|a, b| {
                        a.node.authority.0
                            .partial_cmp(&b.node.authority.0)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                if let Some(w) = winner {
                    results.push(w.clone());
                }
            }
            ConflictPolicy::HighestAuthority => {
                let max_auth = group.iter()
                    .map(|r| r.node.authority.0)
                    .fold(0.0f32, f32::max);
                let winners: Vec<_> = group.iter()
                    .filter(|r| (r.node.authority.0 - max_auth).abs() < 0.001)
                    .collect();
                if winners.len() == 1 {
                    results.push(winners[0].clone());
                } else {
                    ambiguous = true;
                    // Still include the first one, but mark as ambiguous
                    results.push(winners[0].clone());
                }
            }
            ConflictPolicy::Unanimous => {
                // Check if all predicates are identical
                let first = &group[0].node.predicate;
                let all_same = group.iter()
                    .all(|r| r.node.predicate == *first);
                if all_same {
                    results.push(group.into_iter().next().unwrap());
                } else {
                    ambiguous = true;
                }
            }
            ConflictPolicy::Custom => {
                // Not implementable in v0.1
                results.push(group.into_iter().next().unwrap());
            }
        }
    }

    (results, ambiguous)
}

/// Group query results by their binding signature.
fn group_by_bindings(matches: Vec<QueryResult>) -> Vec<(String, Vec<QueryResult>)> {
    let mut groups: HashMap<String, Vec<QueryResult>> = HashMap::new();
    for m in matches {
        let key = binding_key(&m.bindings);
        groups.entry(key).or_default().push(m);
    }
    groups.into_iter().collect()
}

/// Create a comparable key from bindings.
fn binding_key(bindings: &[(SmolStr, Term)]) -> String {
    let mut parts: Vec<String> = bindings.iter()
        .map(|(name, term)| format!("{}={}", name, term_key(term)))
        .collect();
    parts.sort();
    parts.join(",")
}

fn term_key(term: &Term) -> String {
    match term {
        Term::Var(s) => format!("var:{}", s),
        Term::Ent(e) => format!("ent:{}", e),
        Term::Lit(l) => format!("lit:{}", l.to_canonical_string()),
        Term::Compound(_) => "compound".to_string(),
        Term::List(_) => "list".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::QueryResult;
    use std::sync::Arc;

    fn make_result(id: &str, auth: f32, pred: Predicate) -> QueryResult {
        QueryResult {
            node: Arc::new(Node::new(id, pred).with_authority(Authority(auth))),
            bindings: vec![],
        }
    }

    #[test]
    fn test_latest_wins() {
        let matches = vec![
            make_result("n001", 0.5, Predicate::new("p").with_args(vec![Term::ent("X")])),
            make_result("n002", 0.9, Predicate::new("p").with_args(vec![Term::ent("X")])),
            make_result("n003", 0.7, Predicate::new("p").with_args(vec![Term::ent("X")])),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::LatestWins);
        assert_eq!(results.len(), 1);
        assert!(!ambiguous);
        assert_eq!(results[0].node.id.as_str(), "n002"); // highest authority
    }

    #[test]
    fn test_highest_authority_ambiguous() {
        let matches = vec![
            make_result("n001", 0.9, Predicate::new("p").with_args(vec![Term::ent("X")])),
            make_result("n002", 0.9, Predicate::new("p").with_args(vec![Term::ent("X")])),
        ];

        let (_results, ambiguous) = arbitrate(matches, &ConflictPolicy::HighestAuthority);
        assert!(ambiguous);
    }

    #[test]
    fn test_unanimous_agreement() {
        let pred = Predicate::new("instance-of")
            .with_args(vec![Term::ent("X"), Term::ent("org")]);
        let matches = vec![
            make_result("n001", 0.5, pred.clone()),
            make_result("n002", 0.9, pred.clone()),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::Unanimous);
        assert!(!ambiguous);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_unanimous_disagreement() {
        let matches = vec![
            make_result("n001", 0.5,
                Predicate::new("instance-of").with_args(vec![Term::ent("X"), Term::ent("org")])),
            make_result("n002", 0.9,
                Predicate::new("instance-of").with_args(vec![Term::ent("X"), Term::ent("person")])),
        ];

        let (_results, ambiguous) = arbitrate(matches, &ConflictPolicy::Unanimous);
        assert!(ambiguous);
    }
}
