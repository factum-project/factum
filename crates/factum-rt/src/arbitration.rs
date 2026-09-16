//! Conflict arbitration — resolve multiple nodes matching the same query.
//!
//! ## Policies
//! - `LatestWins`: highest authority wins, tiebreak by most recent validity
//! - `HighestAuthority`: strictly by authority score
//! - `Unanimous`: only return if all sources agree
//! - `WeightedVote`: weighted majority vote by principal (multi-agent)
//! - `Custom`: user-provided function
//!
//! ## Key Design Decision
//! When arbitration cannot uniquely resolve, return `Ambiguous`.
//! **We refuse to answer rather than guess.** This is a core Factum principle.
//!
//! ## Ambiguity Behavior by Policy
//!
//! When a conflict cannot be uniquely resolved, each policy has a **deliberately
//! different** behavior regarding whether a candidate is still returned:
//!
//! | Policy            | Ambiguous flag | Result returned?  | Rationale                        |
//! |-------------------|----------------|-------------------|----------------------------------|
//! | `LatestWins`      | `true`         | Yes (best guess)  | Authority+validity tie, pick one |
//! | `HighestAuthority`| `true`         | Yes (first winner)| Authority tie, return a candidate|
//! | `Unanimous`       | `true`         | No (empty)        | Sources disagree, no answer      |
//! | `WeightedVote`    | `true`         | No (empty)        | No majority, refuse              |
//! | `Custom`          | `true`         | No (empty)        | No function, refuse              |
//!
//! **`LatestWins` and `HighestAuthority`** return a candidate even on ambiguity.
//! The caller receives `results.len() > 0` with `ambiguous = true`, meaning
//! "here is the best candidate, but it is not authoritative — use with caution."
//!
//! **`Unanimous`, `WeightedVote`, and `Custom`** return nothing on ambiguity.
//! The caller receives `results.is_empty()` with `ambiguous = true`, meaning
//! "we could not agree, so we refuse to provide any answer."
//!
//! This asymmetry is intentional: authority-based policies always have a
//! "best" candidate to offer (even if tied), while agreement-based policies
//! have no meaningful candidate when consensus fails.
//!
//! ## WeightedVote (Multi-Agent)
//!
//! `WeightedVote` is designed for multi-agent scenarios where different agents
//! (principals) have different reliability weights. When multiple agents assert
//! conflicting facts, the policy groups them by predicate value and sums the
//! weights of each group. A group wins if its total weight exceeds 50% of the
//! total. If no group reaches 50%, the result is `Ambiguous` (refuse to answer).
//!
//! Weights are derived from the node's provenance `Principal` field. Nodes
//! without a recognizable principal (e.g., `Asserted { by: "system" }`) receive
//! a default weight of 0.5. Nodes with `Extracted` provenance use the model
//! name as the principal for weighting.
//!
//! Example:
//! ```text
//! Agent A (weight 0.5): (status @X active)
//! Agent B (weight 0.3): (status @X active)   → same predicate, combined weight 0.8
//! Agent C (weight 0.2): (status @X inactive) → different predicate, weight 0.2
//! Total weight: 1.0. "active" group has 0.8 > 0.5 → resolved.
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use smol_str::SmolStr;
use factum_core::types::*;
use crate::query::QueryResult;

/// Conflict resolution policy.
#[derive(Clone, Debug, PartialEq)]
pub enum ConflictPolicy {
    /// Default: highest authority wins, tiebreak by most recent validity start.
    LatestWins,
    /// Strictly by authority score.
    HighestAuthority,
    /// Only return if all sources agree (same predicate).
    Unanimous,
    /// Weighted majority vote by principal. Weights are provided as a
    /// `Principal → f32` map. A group wins if its total weight > 50% of total.
    /// If no majority, returns `Ambiguous` (refuse to answer).
    WeightedVote {
        /// Map from principal name to weight (0.0–1.0). Principals not in this
        /// map receive a default weight of 0.5.
        weights: HashMap<String, f32>,
    },
    /// User-provided arbitration function (not implementable in v0.1 const context).
    /// When selected, all multi-node groups are marked Ambiguous — we refuse to
    /// answer rather than silently guess.
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
/// Groups results so that nodes matching the same query pattern position
/// (same head + same ground terms) are in the same group, then applies
/// the policy within each group. If a group cannot be uniquely resolved,
/// marks the result set as ambiguous.
///
/// The `query_pattern` is used to determine which positions are variables
/// (contested) vs ground terms (shared). When `query_pattern` is `None`,
/// falls back to grouping by binding values (legacy behavior, for
/// backward compatibility with tests that call arbitrate directly).
pub fn arbitrate(
    matches: Vec<QueryResult>,
    policy: &ConflictPolicy,
    query_pattern: Option<&Predicate>,
) -> (Vec<QueryResult>, bool) {
    if matches.is_empty() {
        return (Vec::new(), false);
    }

    // Group by pattern structure (head + ground positions), not by
    // variable binding values. This ensures that conflicting values
    // for the same variable land in the same group for arbitration.
    let groups = group_by_pattern(matches, query_pattern);

    let mut results = Vec::new();
    let mut ambiguous = false;

    for (_, group) in groups {
        if group.len() == 1 {
            results.push(group.into_iter().next().unwrap());
            continue;
        }

        match policy {
            ConflictPolicy::LatestWins => {
                // Sub-group by canonical predicate so that nodes with different
                // concrete predicates (e.g., different entities) are separate
                // answers, while nodes with the same predicate (true conflicts)
                // compete for the winner.
                let sub_groups = sub_group_by_predicate(&group);
                for sub in sub_groups {
                    if sub.len() == 1 {
                        results.push(sub.into_iter().next().unwrap());
                        continue;
                    }
                    // Sort by authority desc, then by validity start desc.
                    let winner = sub.iter()
                        .max_by(|a, b| {
                            a.node.authority.0
                                .partial_cmp(&b.node.authority.0)
                                .unwrap_or(std::cmp::Ordering::Equal)
                                .then(validity_recency_cmp(&a.node.validity, &b.node.validity))
                        });
                    if let Some(w) = winner {
                        let w_auth = w.node.authority.0;
                        let w_validity = &w.node.validity;
                        let tied: Vec<_> = sub.iter()
                            .filter(|r| {
                                (r.node.authority.0 - w_auth).abs() < 0.001
                                && validity_recency_cmp(&r.node.validity, w_validity)
                                    == std::cmp::Ordering::Equal
                            })
                            .collect();
                        if tied.len() > 1 {
                            ambiguous = true;
                        }
                        results.push(w.clone());
                    }
                }
            }
            ConflictPolicy::HighestAuthority => {
                let sub_groups = sub_group_by_predicate(&group);
                for sub in sub_groups {
                    if sub.len() == 1 {
                        results.push(sub.into_iter().next().unwrap());
                        continue;
                    }
                    let max_auth = sub.iter()
                        .map(|r| r.node.authority.0)
                        .fold(0.0f32, f32::max);
                    let winners: Vec<_> = sub.iter()
                        .filter(|r| (r.node.authority.0 - max_auth).abs() < 0.001)
                        .collect();
                    if winners.len() == 1 {
                        results.push(winners[0].clone());
                    } else {
                        ambiguous = true;
                        results.push(winners[0].clone());
                    }
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
            ConflictPolicy::WeightedVote { weights } => {
                // Group by predicate canonical form, sum weights per group.
                // A group wins if its total weight > 50% of total weight.
                // If no group reaches majority, mark Ambiguous (refuse).
                let default_weight = 0.5f32;

                // Build predicate_signature → (total_weight, first_result)
                let mut pred_groups: HashMap<String, (f32, QueryResult)> = HashMap::new();
                let mut total_weight = 0.0f32;

                for r in &group {
                    let sig = predicate_signature(&r.node.predicate);
                    let principal_name = principal_str(&r.node.provenance);
                    let w = weights
                        .get(&principal_name)
                        .copied()
                        .unwrap_or(default_weight);
                    total_weight += w;
                    pred_groups
                        .entry(sig)
                        .and_modify(|(tw, _)| *tw += w)
                        .or_insert((w, r.clone()));
                }

                // Find the group with highest total weight
                if let Some((_best_sig, (best_weight, best_result))) =
                    pred_groups.iter().max_by(|(_, (aw, _)), (_, (bw, _))| {
                        aw.partial_cmp(bw).unwrap_or(std::cmp::Ordering::Equal)
                    })
                {
                    // Win condition: best weight > 50% of total
                    if *best_weight > total_weight * 0.5 {
                        results.push(best_result.clone());
                    } else {
                        // No majority — refuse to answer
                        ambiguous = true;
                    }
                } else {
                    ambiguous = true;
                }
            }
            ConflictPolicy::Custom => {
                // Custom arbitration is not implementable in v0.1 (requires a
                // user-provided function, which cannot be stored in a const enum).
                // We refuse to answer rather than silently pick one — this is a
                // core Factum principle.
                ambiguous = true;
            }
        }
    }

    (results, ambiguous)
}

/// Group query results for arbitration.
///
/// The key insight: arbitration should engage when **multiple nodes match
/// the same query pattern** and could be considered conflicting answers.
///
/// We group by the **query pattern's ground structure** (head + ground
/// argument positions + ground named args). Variable positions are
/// excluded from the key, so that nodes with different values for the
/// same variable position land in the same group.
///
/// This means:
/// - `(revenue-trend @ACME-CORP ?trend)` matching `declining` and `growing`
///   → both in group "revenue-trend|ent:ACME-CORP|?" → arbitration engages
/// - `(instance-of ?x organization)` matching `ACME-CORP` and `APPLE`
///   → both in group "instance-of|?|ent:organization" → arbitration engages
///
/// The second case is correct: if two agents both assert `(instance-of @X org)`
/// and `(instance-of @X person)`, that IS a conflict. But if they assert
/// different entities (`@ACME-CORP` vs `@APPLE`), the canonical predicate
/// differs and the WeightedVote/Unanimous sub-grouping by predicate handles it.
///
/// For LatestWins/HighestAuthority, all nodes in the same group compete
/// and the winner is selected by authority/validity. For multi-entity
/// queries, this means the highest-authority node wins per ground pattern —
/// which is the intended behavior (return the most authoritative answer).
fn group_by_pattern(
    matches: Vec<QueryResult>,
    pattern: Option<&Predicate>,
) -> Vec<(String, Vec<QueryResult>)> {
    let Some(pat) = pattern else {
        // Fallback: legacy binding-based grouping (for direct arbitrate() calls)
        return group_by_bindings(matches);
    };

    // Extract the pattern's ground positions
    let pat_head = match &pat.head {
        PredicateHead::Name(n) => n.as_str(),
        PredicateHead::Id(_) => "<morpheme-id>",
    };

    // Build the group key from head + ground (non-variable) arg positions.
    // Variable positions are represented as "?" in the key (same for all
    // nodes matching that pattern position).
    let mut ground_key_parts: Vec<String> = vec![pat_head.to_string()];

    for pat_arg in &pat.args {
        match pat_arg {
            Term::Var(_) => {
                ground_key_parts.push("?".to_string());
            }
            _ => {
                ground_key_parts.push(term_key(pat_arg));
            }
        }
    }

    // For named args: ground keys use the pattern's named arg keys
    let mut named_keys: Vec<String> = vec![];
    for (key, val) in &pat.named {
        match val {
            Term::Var(_) => named_keys.push(format!("{}=?", key)),
            _ => named_keys.push(format!("{}={}", key, term_key(val))),
        }
    }
    named_keys.sort();
    ground_key_parts.extend(named_keys);

    let group_key = ground_key_parts.join("|");

    // All matches that share this pattern structure go in one group
    let mut groups: HashMap<String, Vec<QueryResult>> = HashMap::new();
    for m in matches {
        groups
            .entry(group_key.clone())
            .or_default()
            .push(m);
    }
    groups.into_iter().collect()
}

/// Sub-group results within a group by their canonical predicate.
/// Nodes with the same predicate (true duplicates/conflicts) compete;
/// nodes with different predicates (different facts) are separate answers.
fn sub_group_by_predicate(group: &[QueryResult]) -> Vec<Vec<QueryResult>> {
    let mut sub_groups: HashMap<String, Vec<QueryResult>> = HashMap::new();
    for r in group {
        let sig = predicate_signature(&r.node.predicate);
        sub_groups.entry(sig).or_default().push(r.clone());
    }
    sub_groups.into_values().collect()
}

/// Group query results by their binding signature (legacy fallback).
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

/// Compare two validities by recency (most recent wins).
///
/// Returns `Greater` if `a` is more recent than `b`.
/// `Forever` is treated as the least recent (earliest possible start),
/// so any `Window` with a finite `from` is more recent than `Forever`.
/// When both are `Window`, compares by `from` timestamp.
fn validity_recency_cmp(a: &Validity, b: &Validity) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Validity::Forever, Validity::Forever) => Ordering::Equal,
        (Validity::Forever, Validity::Window { .. }) => Ordering::Less,
        (Validity::Window { .. }, Validity::Forever) => Ordering::Greater,
        (Validity::Window { from: a_from, .. }, Validity::Window { from: b_from, .. }) => {
            a_from.cmp(b_from)
        }
    }
}

/// Create a canonical signature string for a predicate, used for grouping
/// in WeightedVote. Two predicates with the same head, same args, and same
/// named args produce the same signature.
fn predicate_signature(pred: &Predicate) -> String {
    use factum_core::serialize::canonical_predicate;
    canonical_predicate(pred)
}

/// Extract the principal name from a provenance for WeightedVote weighting.
///
/// - `Asserted { by }` → `by.0` (the principal name)
/// - `Extracted { model, .. }` → `model.name` (the model that extracted it)
/// - `Verbatim` / `Summary` → `"document"` (no principal)
/// - `Derived { from, rule }` → `"derived"` (no principal)
fn principal_str(p: &Provenance) -> String {
    match p {
        Provenance::Asserted { by } => by.0.to_string(),
        Provenance::Extracted { model, .. } => model.name.to_string(),
        Provenance::Verbatim { .. } => "document".to_string(),
        Provenance::Summary { .. } => "document".to_string(),
        Provenance::Derived { .. } => "derived".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::QueryResult;
    use std::sync::Arc;
    use chrono::{TimeZone, Utc};
    use factum_core::types::Validity;

    fn make_result(id: &str, auth: f32, pred: Predicate) -> QueryResult {
        QueryResult {
            node: Arc::new(Node::new(id, pred).with_authority(Authority(auth))),
            bindings: vec![],
        }
    }

    fn make_result_with_validity(
        id: &str,
        auth: f32,
        pred: Predicate,
        validity: Validity,
    ) -> QueryResult {
        QueryResult {
            node: Arc::new(
                Node::new(id, pred)
                    .with_authority(Authority(auth))
                    .with_validity(validity),
            ),
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

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::LatestWins, None);
        assert_eq!(results.len(), 1);
        assert!(!ambiguous);
        assert_eq!(results[0].node.id.as_str(), "n002"); // highest authority
    }

    // ── Bug 1 regression: LatestWins must tiebreak by validity start ──

    #[test]
    fn test_latest_wins_validity_tiebreak() {
        // Two nodes with same authority, different validity starts.
        // The one with the more recent validity start should win.
        let pred = Predicate::new("p").with_args(vec![Term::ent("X")]);
        let older = make_result_with_validity(
            "n001", 0.9, pred.clone(),
            Validity::Window {
                from: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                until: None,
            },
        );
        let newer = make_result_with_validity(
            "n002", 0.9, pred.clone(),
            Validity::Window {
                from: Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap(),
                until: None,
            },
        );

        let (results, ambiguous) = arbitrate(vec![older, newer], &ConflictPolicy::LatestWins, None);
        assert!(!ambiguous);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node.id.as_str(), "n002"); // more recent validity wins
    }

    #[test]
    fn test_latest_wins_window_beats_forever() {
        // Same authority: Window (finite start) should beat Forever.
        let pred = Predicate::new("p").with_args(vec![Term::ent("X")]);
        let forever = make_result("n001", 0.9, pred.clone());
        let window = make_result_with_validity(
            "n002", 0.9, pred.clone(),
            Validity::Window {
                from: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
                until: None,
            },
        );

        let (results, ambiguous) = arbitrate(vec![forever, window], &ConflictPolicy::LatestWins, None);
        assert!(!ambiguous);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node.id.as_str(), "n002"); // Window beats Forever
    }

    #[test]
    fn test_latest_wins_tiebreak_tie_is_ambiguous() {
        // Same authority AND same validity start → cannot pick, must be ambiguous.
        let pred = Predicate::new("p").with_args(vec![Term::ent("X")]);
        let v = Validity::Window {
            from: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
            until: None,
        };
        let a = make_result_with_validity("n001", 0.9, pred.clone(), v);
        let b = make_result_with_validity("n002", 0.9, pred.clone(), v);

        let (results, ambiguous) = arbitrate(vec![a, b], &ConflictPolicy::LatestWins, None);
        assert!(ambiguous, "same authority + same validity must be ambiguous");
        // Results still contains one entry (the winner), but ambiguous flag is set
        assert_eq!(results.len(), 1);
    }

    // ── Bug 2 regression: Custom must mark Ambiguous, not silently guess ──

    #[test]
    fn test_custom_policy_marks_ambiguous() {
        let pred = Predicate::new("p").with_args(vec![Term::ent("X")]);
        let matches = vec![
            make_result("n001", 0.5, pred.clone()),
            make_result("n002", 0.9, pred.clone()),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::Custom, None);
        assert!(ambiguous, "Custom policy must mark ambiguous, not silently guess");
        assert!(results.is_empty(), "Custom policy must not push any result");
    }

    #[test]
    fn test_custom_policy_single_result_still_resolved() {
        // Single-node group should still pass through (no conflict to arbitrate).
        let pred = Predicate::new("p").with_args(vec![Term::ent("X")]);
        let matches = vec![make_result("n001", 0.5, pred.clone())];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::Custom, None);
        assert!(!ambiguous);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_highest_authority_ambiguous() {
        let matches = vec![
            make_result("n001", 0.9, Predicate::new("p").with_args(vec![Term::ent("X")])),
            make_result("n002", 0.9, Predicate::new("p").with_args(vec![Term::ent("X")])),
        ];

        // HighestAuthority on tie: returns a candidate (winners[0]) AND marks ambiguous.
        // This is deliberately different from Unanimous (which returns nothing).
        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::HighestAuthority, None);
        assert!(ambiguous);
        assert_eq!(results.len(), 1, "HighestAuthority must still return a candidate on tie");
    }

    #[test]
    fn test_unanimous_agreement() {
        let pred = Predicate::new("instance-of")
            .with_args(vec![Term::ent("X"), Term::ent("org")]);
        let matches = vec![
            make_result("n001", 0.5, pred.clone()),
            make_result("n002", 0.9, pred.clone()),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::Unanimous, None);
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

        // Unanimous on disagreement: marks ambiguous AND returns no result.
        // This is deliberately different from HighestAuthority (which returns a candidate).
        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::Unanimous, None);
        assert!(ambiguous);
        assert!(results.is_empty(), "Unanimous must not return any result on disagreement");
    }

    // ── WeightedVote tests (multi-agent conflict resolution) ──

    fn make_result_with_provenance(
        id: &str,
        auth: f32,
        pred: Predicate,
        prov: Provenance,
    ) -> QueryResult {
        QueryResult {
            node: Arc::new(
                Node::new(id, pred)
                    .with_authority(Authority(auth))
                    .with_provenance(prov),
            ),
            bindings: vec![],
        }
    }

    fn asserted_by(name: &str) -> Provenance {
        Provenance::Asserted {
            by: Principal(SmolStr::new(name)),
        }
    }

    #[test]
    fn test_weighted_vote_majority_wins() {
        // Agent A (weight 0.5) and Agent B (weight 0.3) agree: status=active
        // Agent C (weight 0.2) disagrees: status=inactive
        // "active" group total = 0.8 > 0.5*1.0 = 0.5 → resolved
        let mut weights = HashMap::new();
        weights.insert("agent-a".to_string(), 0.5);
        weights.insert("agent-b".to_string(), 0.3);
        weights.insert("agent-c".to_string(), 0.2);

        let pred_active = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("active")]);
        let pred_inactive = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("inactive")]);

        let matches = vec![
            make_result_with_provenance("n001", 0.5, pred_active.clone(), asserted_by("agent-a")),
            make_result_with_provenance("n002", 0.5, pred_active.clone(), asserted_by("agent-b")),
            make_result_with_provenance("n003", 0.5, pred_inactive.clone(), asserted_by("agent-c")),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::WeightedVote { weights }, None);
        assert!(!ambiguous, "majority should resolve");
        assert_eq!(results.len(), 1);
        // Winner should be one of the "active" nodes
        assert!(
            results[0].node.id.as_str() == "n001" || results[0].node.id.as_str() == "n002",
            "winner should be an 'active' node, got: {}", results[0].node.id
        );
    }

    #[test]
    fn test_weighted_vote_no_majority_is_ambiguous() {
        // Agent A (weight 0.4): active
        // Agent B (weight 0.4): inactive
        // Agent C (weight 0.2): active
        // "active" group total = 0.6, "inactive" = 0.4, total = 1.0
        // 0.6 > 0.5 → actually resolves. Let's make it fail:
        // Agent A (0.4): active, Agent B (0.4): inactive, Agent C (0.2): pending
        // No group > 0.5 → ambiguous
        let mut weights = HashMap::new();
        weights.insert("agent-a".to_string(), 0.4);
        weights.insert("agent-b".to_string(), 0.4);
        weights.insert("agent-c".to_string(), 0.2);

        let pred_a = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("active")]);
        let pred_b = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("inactive")]);
        let pred_c = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("pending")]);

        let matches = vec![
            make_result_with_provenance("n001", 0.5, pred_a, asserted_by("agent-a")),
            make_result_with_provenance("n002", 0.5, pred_b, asserted_by("agent-b")),
            make_result_with_provenance("n003", 0.5, pred_c, asserted_by("agent-c")),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::WeightedVote { weights }, None);
        assert!(ambiguous, "no majority must be ambiguous");
        assert!(results.is_empty(), "WeightedVote must not return result on no majority");
    }

    #[test]
    fn test_weighted_vote_all_agree_resolves() {
        // All agents agree → resolves (like Unanimous but via weight)
        let mut weights = HashMap::new();
        weights.insert("agent-a".to_string(), 0.5);
        weights.insert("agent-b".to_string(), 0.3);

        let pred = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("active")]);

        let matches = vec![
            make_result_with_provenance("n001", 0.5, pred.clone(), asserted_by("agent-a")),
            make_result_with_provenance("n002", 0.7, pred.clone(), asserted_by("agent-b")),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::WeightedVote { weights }, None);
        assert!(!ambiguous, "all agree must resolve");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_weighted_vote_default_weight_for_unknown_principal() {
        // Unknown principals get default weight 0.5
        // Agent X (unknown, weight 0.5): active
        // Agent Y (unknown, weight 0.5): inactive
        // Neither > 0.5 → ambiguous
        let weights = HashMap::new(); // empty → all use default 0.5

        let pred_a = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("active")]);
        let pred_b = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("inactive")]);

        let matches = vec![
            make_result_with_provenance("n001", 0.5, pred_a, asserted_by("unknown-x")),
            make_result_with_provenance("n002", 0.5, pred_b, asserted_by("unknown-y")),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::WeightedVote { weights }, None);
        assert!(ambiguous, "equal weights with disagreement must be ambiguous");
        assert!(results.is_empty());
    }

    #[test]
    fn test_weighted_vote_single_node_passes_through() {
        // Single-node group should pass through regardless of policy
        let weights = HashMap::new();
        let pred = Predicate::new("status")
            .with_args(vec![Term::ent("X"), Term::ent("active")]);

        let matches = vec![
            make_result_with_provenance("n001", 0.5, pred, asserted_by("agent-a")),
        ];

        let (results, ambiguous) = arbitrate(matches, &ConflictPolicy::WeightedVote { weights }, None);
        assert!(!ambiguous);
        assert_eq!(results.len(), 1);
    }
}
