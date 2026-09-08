//! Query engine for Factum.
//!
//! Queries find nodes matching a predicate pattern with optional
//! variable bindings, filtered by validity, permission, and conflict policy.

use std::sync::Arc;
use chrono::{DateTime, Utc};
use smol_str::SmolStr;
use factum_core::types::*;
use crate::store::FactumStore;
use crate::arbitration::ConflictPolicy;
use crate::permission::PermissionContext;

/// A query against the Factum store.
#[derive(Clone, Debug)]
pub struct Query {
    /// Predicate pattern to match. Variables act as wildcards.
    pub pattern: Predicate,
    /// Optional entity filter (quick index lookup).
    pub entity_filter: Option<EntityId>,
}

impl Query {
    pub fn new(pattern: Predicate) -> Self {
        Self { pattern, entity_filter: None }
    }

    pub fn with_entity_filter(mut self, entity: EntityId) -> Self {
        self.entity_filter = Some(entity);
        self
    }
}

/// Query options: temporal, conflict, permission.
#[derive(Clone, Debug)]
pub struct QueryOptions {
    /// Conflict resolution policy.
    pub policy: ConflictPolicy,
    /// "As of" historical query time. Defaults to now.
    pub now: DateTime<Utc>,
    /// Minimum confidence threshold.
    pub min_conf: Confidence,
    /// Queryer's permission context.
    pub perm: PermissionContext,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            policy: ConflictPolicy::LatestWins,
            now: Utc::now(),
            min_conf: Confidence(0.0),
            perm: PermissionContext::default(),
        }
    }
}

/// A single query result.
#[derive(Clone, Debug)]
pub struct QueryResult {
    pub node: Arc<Node>,
    /// Variable bindings from the match (var name → matched term).
    pub bindings: Vec<(SmolStr, Term)>,
}

/// A set of query results.
#[derive(Clone, Debug, Default)]
pub struct ResultSet {
    pub results: Vec<QueryResult>,
    /// Whether any results were filtered out due to ambiguity.
    pub ambiguous: bool,
}

/// Query errors.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("permission denied")]
    PermissionDenied,
    #[error("invalid query: {0}")]
    InvalidQuery(String),
}

impl FactumStore {
    /// Execute a query with the given options.
    ///
    /// Flow:
    /// 1. Get candidate nodes (by entity filter or predicate head)
    /// 2. Filter by permission (INDEX LEVEL, not post-query)
    /// 3. Filter by validity (at `options.now`)
    /// 4. Filter by confidence threshold
    /// 5. Match pattern against each candidate
    /// 6. Arbitrate conflicts (multiple nodes matching same binding)
    pub fn query(&self, q: &Query, options: &QueryOptions) -> Result<ResultSet, QueryError> {
        // 1. Get candidates
        let candidates = self.get_candidates(q);

        // 2. Permission + validity + confidence filter
        let filtered: Vec<Arc<Node>> = candidates.into_iter()
            .filter(|n| {
                // Permission filter (index level — this is the critical design decision)
                if !self.check_permission(n, &options.perm) {
                    return false;
                }
                // Validity filter
                if !n.is_active_valid_at(options.now) {
                    return false;
                }
                // Confidence filter
                if n.confidence < options.min_conf {
                    return false;
                }
                true
            })
            .collect();

        // 3. Pattern matching
        let mut matches: Vec<QueryResult> = Vec::new();
        for node in &filtered {
            if let Some(bindings) = match_pattern(&q.pattern, &node.predicate) {
                matches.push(QueryResult {
                    node: node.clone(),
                    bindings,
                });
            }
        }

        // 4. Conflict arbitration
        let (arbitrated, ambiguous) = crate::arbitration::arbitrate(matches, &options.policy);

        Ok(ResultSet { results: arbitrated, ambiguous })
    }

    /// Get candidate nodes for a query.
    fn get_candidates(&self, q: &Query) -> Vec<Arc<Node>> {
        // If entity filter is set, use index
        if let Some(entity) = &q.entity_filter {
            return self.lookup_by_entity(entity);
        }

        // Otherwise, use predicate head index
        let head_str = match &q.pattern.head {
            PredicateHead::Name(name) => name.to_string(),
            PredicateHead::Id(id) => {
                self.registry().lookup_id(*id)
                    .map(|d| d.name.to_string())
                    .unwrap_or_else(|| format!("M{}", id.0))
            }
        };
        self.lookup_by_pred(&head_str)
    }
}

/// Match a query pattern against a concrete predicate.
/// Returns variable bindings if the pattern matches.
fn match_pattern(pattern: &Predicate, concrete: &Predicate) -> Option<Vec<(SmolStr, Term)>> {
    // Heads must match
    let pattern_head_name = match &pattern.head {
        PredicateHead::Name(n) => n.as_str(),
        PredicateHead::Id(_id) => return None, // ID matching not implemented in v0.1
    };
    let concrete_head_name = match &concrete.head {
        PredicateHead::Name(n) => n.as_str(),
        PredicateHead::Id(_) => return None,
    };
    if pattern_head_name != concrete_head_name {
        return None;
    }

    let mut bindings = Vec::new();

    // Match positional args
    if pattern.args.len() != concrete.args.len() {
        return None;
    }
    for (pat, conc) in pattern.args.iter().zip(&concrete.args) {
        if !match_term(pat, conc, &mut bindings) {
            return None;
        }
    }

    // Match named args
    for (pat_key, pat_val) in &pattern.named {
        let conc_val = concrete.named.iter()
            .find(|(k, _)| k == pat_key)
            .map(|(_, v)| v);
        {
            let cv = conc_val?;
            if !match_term(pat_val, cv, &mut bindings) {
                return None;
            }
        }
    }

    Some(bindings)
}

/// Match a pattern term against a concrete term.
fn match_term(pattern: &Term, concrete: &Term, bindings: &mut Vec<(SmolStr, Term)>) -> bool {
    match pattern {
        Term::Var(name) => {
            // Variable matches anything; check if already bound
            if let Some((_, existing)) = bindings.iter().find(|(n, _)| n == name) {
                // Same variable must bind to same value
                existing == concrete
            } else {
                bindings.push((name.clone(), concrete.clone()));
                true
            }
        }
        Term::Ent(e) => {
            matches!(concrete, Term::Ent(ce) if ce == e)
        }
        Term::Lit(l) => {
            matches!(concrete, Term::Lit(cl) if cl == l)
        }
        Term::Compound(pred) => {
            if let Term::Compound(cp) = concrete {
                // Recursively match
                match_pattern(pred, cp).is_some()
            } else {
                false
            }
        }
        Term::List(items) => {
            if let Term::List(ci) = concrete {
                if items.len() != ci.len() { return false; }
                items.iter().zip(ci).all(|(p, c)| match_term(p, c, bindings))
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> FactumStore {
        let store = FactumStore::with_seeds();
        store.insert(Node::new("n001",
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("ACME-CORP"), Term::ent("organization")])).with_permissions(PermissionTag::PUBLIC)).unwrap();
        store.insert(Node::new("n002",
            Predicate::new("located-in")
                .with_args(vec![Term::ent("ACME-CORP"), Term::ent("ACME-HQ")])).with_permissions(PermissionTag::PUBLIC)).unwrap();
        store.insert(Node::new("n003",
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("APPLE"), Term::ent("organization")])).with_permissions(PermissionTag::PUBLIC)).unwrap();
        store
    }

    #[test]
    fn test_query_basic() {
        let store = make_store();
        let q = Query::new(
            Predicate::new("instance-of")
                .with_args(vec![Term::var("x"), Term::ent("organization")])
        );
        let results = store.query(&q, &QueryOptions::default()).unwrap();
        assert_eq!(results.results.len(), 2);
    }

    #[test]
    fn test_query_with_var_binding() {
        let store = make_store();
        let q = Query::new(
            Predicate::new("located-in")
                .with_args(vec![Term::ent("ACME-CORP"), Term::var("loc")])
        );
        let results = store.query(&q, &QueryOptions::default()).unwrap();
        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].bindings.len(), 1);
        // The binding should map "loc" to "ACME-HQ"
        let (_, val) = &results.results[0].bindings[0];
        match val {
            Term::Ent(e) => assert_eq!(e.as_str(), "ACME-HQ"),
            _ => panic!("expected entity"),
        }
    }

    #[test]
    fn test_query_entity_filter() {
        let store = make_store();
        let q = Query::new(
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("ACME-CORP"), Term::var("type")])
        ).with_entity_filter(EntityId::new("ACME-CORP"));

        let results = store.query(&q, &QueryOptions::default()).unwrap();
        assert_eq!(results.results.len(), 1);
    }

    #[test]
    fn test_query_no_match() {
        let store = make_store();
        let q = Query::new(
            Predicate::new("nonexistent")
                .with_args(vec![Term::var("x")])
        );
        let results = store.query(&q, &QueryOptions::default()).unwrap();
        assert_eq!(results.results.len(), 0);
    }
}
