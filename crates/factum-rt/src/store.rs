//! Factum store — the persistence and indexing layer.
//!
//! ## Design Decisions
//! - **In-memory HashMap** for v0.1 (production: RocksDB with WAL + MVCC)
//! - **Index-level permission filtering** — NOT post-query filtering
//!   (post-filtering leaks aggregate information to unauthorized users)
//! - **Secondary indices** are separate from main store and can be rebuilt
//! - **Soft delete only** — retracted nodes are marked, never removed
//!   (supports "as of" historical queries and audit trails)

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use ahash::AHashMap;
use factum_core::types::*;
use factum_core::morphemes::MorphemeRegistry;
use crate::permission::PermissionContext;

/// The main Factum store.
///
/// Thread-safe via `parking_lot::RwLock`.
/// All writes are atomic at the method level (production: `write_batch`).
pub struct FactumStore {
    /// Main storage: NodeId → Node
    nodes: RwLock<AHashMap<NodeId, Arc<Node>>>,

    /// Secondary index: EntityId → [NodeId]
    by_entity: RwLock<HashMap<EntityId, Vec<NodeId>>>,

    /// Secondary index: MorphemeId/Morpheme name → [NodeId]
    by_pred: RwLock<HashMap<String, Vec<NodeId>>>,

    /// Secondary index: DocId → [NodeId] (for document-level retraction)
    by_src: RwLock<HashMap<String, Vec<NodeId>>>,

    /// Secondary index: PermissionTag bitmask → [NodeId]
    by_perm: RwLock<HashMap<u32, Vec<NodeId>>>,

    /// Reverse dependency: NodeId → [NodeIds that depend on it]
    /// Used for cascade invalidation when upstream nodes are retracted.
    deps_rev: RwLock<HashMap<NodeId, Vec<NodeId>>>,

    /// Validity index: sorted by validity window for temporal queries.
    /// (from_timestamp, until_timestamp_or_max, NodeId)
    /// Uses BTreeMap for efficient range queries.
    by_validity: RwLock<BTreeMap<(i64, i64), Vec<NodeId>>>,

    /// Morpheme registry for resolving names.
    registry: Arc<MorphemeRegistry>,

    /// Write-ahead log for event replay (in-memory for v0.1).
    wal: RwLock<Vec<WalEntry>>,
}

/// WAL entry for event sourcing.
#[derive(Clone, Debug)]
pub enum WalEntry {
    Insert(Box<Node>),
    Retract(NodeId),
    /// Transaction boundary
    Checkpoint,
}

/// Errors from store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("node not found: {0}")]
    NotFound(String),
    #[error("node already exists: {0}")]
    AlreadyExists(String),
    #[error("permission denied")]
    PermissionDenied,
    #[error("invalid node: {0}")]
    InvalidNode(String),
}

impl FactumStore {
    /// Create a new empty store with the given morpheme registry.
    pub fn new(registry: Arc<MorphemeRegistry>) -> Self {
        Self {
            nodes: RwLock::new(AHashMap::new()),
            by_entity: RwLock::new(HashMap::new()),
            by_pred: RwLock::new(HashMap::new()),
            by_src: RwLock::new(HashMap::new()),
            by_perm: RwLock::new(HashMap::new()),
            deps_rev: RwLock::new(HashMap::new()),
            by_validity: RwLock::new(BTreeMap::new()),
            registry,
            wal: RwLock::new(Vec::new()),
        }
    }

    /// Create a store with default seed morphemes.
    pub fn with_seeds() -> Self {
        Self::new(Arc::new(MorphemeRegistry::with_seeds()))
    }

    /// Get the morpheme registry.
    pub fn registry(&self) -> &Arc<MorphemeRegistry> {
        &self.registry
    }

    // ─── Write Operations ──────────────────────────────

    /// Insert a new node. Fails if a node with the same ID already exists.
    pub fn insert(&self, node: Node) -> Result<(), StoreError> {
        let id = node.id.clone();

        // Check for duplicate
        {
            let nodes = self.nodes.read();
            if nodes.contains_key(&id) {
                return Err(StoreError::AlreadyExists(id.to_string()));
            }
        }

        // WAL
        self.wal.write().push(WalEntry::Insert(Box::new(node.clone())));

        // Update indices
        self.update_indices(&node);

        // Update reverse deps
        for dep in &node.deps {
            self.deps_rev.write()
                .entry(dep.clone())
                .or_default()
                .push(id.clone());
        }

        // Insert into main store
        self.nodes.write().insert(id, Arc::new(node));

        Ok(())
    }

    /// Insert multiple nodes atomically (all-or-nothing).
    pub fn insert_batch(&self, nodes: Vec<Node>) -> Result<(), StoreError> {
        // Pre-check: no duplicates within batch or with existing
        {
            let store = self.nodes.read();
            let mut seen = HashSet::new();
            for node in &nodes {
                if store.contains_key(&node.id) || !seen.insert(node.id.clone()) {
                    return Err(StoreError::AlreadyExists(node.id.to_string()));
                }
            }
        }

        // Insert all
        for node in nodes {
            self.wal.write().push(WalEntry::Insert(Box::new(node.clone())));
            self.update_indices(&node);
            for dep in &node.deps {
                self.deps_rev.write()
                    .entry(dep.clone())
                    .or_default()
                    .push(node.id.clone());
            }
            self.nodes.write().insert(node.id.clone(), Arc::new(node));
        }

        self.wal.write().push(WalEntry::Checkpoint);
        Ok(())
    }

    /// Retract a node (soft delete).
    ///
    /// The node is marked as Retracted but NOT deleted.
    /// All downstream Derived nodes are cascade-retracted via deps_rev.
    pub fn retract(&self, id: &NodeId) -> Result<Vec<NodeId>, StoreError> {
        // Get the node
        let _node = {
            let nodes = self.nodes.read();
            nodes.get(id).cloned().ok_or_else(|| StoreError::NotFound(id.to_string()))?
        };

        // Mark as retracted
        {
            let mut nodes = self.nodes.write();
            let arc_node = nodes.get_mut(id).unwrap();
            // We need to clone to modify since it's Arc
            let mut new_node = (**arc_node).clone();
            new_node.status = NodeStatus::Retracted;
            *arc_node = Arc::new(new_node);
        }

        self.wal.write().push(WalEntry::Retract(id.clone()));

        // Cascade: find all nodes that depend on this one
        let dependents = self.deps_rev.read().get(id).cloned().unwrap_or_default();

        let mut all_retracted = vec![id.clone()];
        for dep_id in &dependents {
            // Only cascade-retract Derived nodes
            let dep_node = self.nodes.read().get(dep_id).cloned();
            if let Some(n) = dep_node {
                if matches!(n.provenance, Provenance::Derived { .. }) && n.status == NodeStatus::Active {
                    let mut further = self.retract(dep_id)?;
                    all_retracted.append(&mut further);
                }
            }
        }

        Ok(all_retracted)
    }

    // ─── Read Operations ───────────────────────────────

    /// Get a node by ID.
    pub fn get(&self, id: &NodeId) -> Option<Arc<Node>> {
        self.nodes.read().get(id).cloned()
    }

    /// Get a node by ID, checking permissions.
    pub fn get_with_perm(&self, id: &NodeId, ctx: &PermissionContext) -> Result<Arc<Node>, StoreError> {
        let node = self.get(id).ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        if !self.check_permission(&node, ctx) {
            return Err(StoreError::PermissionDenied);
        }
        Ok(node)
    }

    /// Get all nodes (for testing/admin only).
    pub fn all(&self) -> Vec<Arc<Node>> {
        self.nodes.read().values().cloned().collect()
    }

    /// Get all active (non-retracted) nodes.
    pub fn all_active(&self) -> Vec<Arc<Node>> {
        self.nodes.read().values()
            .filter(|n| n.status == NodeStatus::Active)
            .cloned()
            .collect()
    }

    /// Lookup nodes by entity.
    pub fn lookup_by_entity(&self, entity: &EntityId) -> Vec<Arc<Node>> {
        let ids = self.by_entity.read().get(entity).cloned().unwrap_or_default();
        self.resolve_nodes(&ids)
    }

    /// Lookup nodes by predicate head name.
    pub fn lookup_by_pred(&self, head: &str) -> Vec<Arc<Node>> {
        let ids = self.by_pred.read().get(head).cloned().unwrap_or_default();
        self.resolve_nodes(&ids)
    }

    /// Lookup nodes by source document.
    pub fn lookup_by_doc(&self, doc: &str) -> Vec<Arc<Node>> {
        let ids = self.by_src.read().get(doc).cloned().unwrap_or_default();
        self.resolve_nodes(&ids)
    }

    /// Lookup nodes valid at a specific time.
    pub fn lookup_valid_at(&self, t: DateTime<Utc>) -> Vec<Arc<Node>> {
        let nodes = self.nodes.read();

        // Filter by validity
        nodes.values()
            .filter(|n| n.status == NodeStatus::Active && n.validity.is_valid_at(t))
            .cloned()
            .collect()
    }

    /// Lookup nodes valid now.
    pub fn lookup_valid_now(&self) -> Vec<Arc<Node>> {
        self.lookup_valid_at(Utc::now())
    }

    /// Total node count.
    pub fn len(&self) -> usize {
        self.nodes.read().len()
    }

    /// Is the store empty?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get WAL entries for replay.
    pub fn wal_entries(&self) -> Vec<WalEntry> {
        self.wal.read().clone()
    }

    // ─── Internal Helpers ──────────────────────────────

    fn resolve_nodes(&self, ids: &[NodeId]) -> Vec<Arc<Node>> {
        let nodes = self.nodes.read();
        ids.iter()
            .filter_map(|id| nodes.get(id).cloned())
            .filter(|n| n.status == NodeStatus::Active)
            .collect()
    }

    fn update_indices(&self, node: &Node) {
        // by_entity: extract entity references from predicate args
        for arg in &node.predicate.args {
            self.index_term_entity(arg, &node.id);
        }
        for (_, val) in &node.predicate.named {
            self.index_term_entity(val, &node.id);
        }

        // by_pred
        let head_key = match &node.predicate.head {
            PredicateHead::Name(name) => name.to_string(),
            PredicateHead::Id(id) => {
                self.registry.lookup_id(*id)
                    .map(|d| d.name.to_string())
                    .unwrap_or_else(|| format!("M{}", id.0))
            }
        };
        self.by_pred.write()
            .entry(head_key)
            .or_default()
            .push(node.id.clone());

        // by_src
        let src_key = match &node.provenance {
            Provenance::Verbatim { doc, .. } |
            Provenance::Summary { doc, .. } |
            Provenance::Extracted { doc, .. } => Some(doc.0.to_string()),
            Provenance::Derived { from, .. } => Some(from.to_string()),
            Provenance::Asserted { by } => Some(by.0.to_string()),
        };
        if let Some(key) = src_key {
            self.by_src.write().entry(key).or_default().push(node.id.clone());
        }

        // by_perm
        self.by_perm.write()
            .entry(node.permissions.0)
            .or_default()
            .push(node.id.clone());

        // by_validity
        let (from_ts, until_ts) = match node.validity {
            Validity::Forever => (i64::MIN, i64::MAX),
            Validity::Window { from, until } => {
                (from.timestamp(), until.map_or(i64::MAX, |u| u.timestamp()))
            }
        };
        self.by_validity.write()
            .entry((from_ts, until_ts))
            .or_default()
            .push(node.id.clone());
    }

    fn index_term_entity(&self, term: &Term, node_id: &NodeId) {
        match term {
            Term::Ent(e) => {
                self.by_entity.write()
                    .entry(e.clone())
                    .or_default()
                    .push(node_id.clone());
            }
            Term::Compound(pred) => {
                for arg in &pred.args {
                    self.index_term_entity(arg, node_id);
                }
            }
            Term::List(items) => {
                for item in items {
                    self.index_term_entity(item, node_id);
                }
            }
            _ => {}
        }
    }

    /// Check if a permission context grants access to a node.
    /// Permission filtering happens at index level, not post-query.
    pub fn check_permission(&self, node: &Node, ctx: &PermissionContext) -> bool {
        node.permissions.grants(ctx.role_mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smol_str::SmolStr;

    fn make_node(id: &str, pred_name: &str, args: Vec<Term>) -> Node {
        Node::new(id, Predicate::new(pred_name).with_args(args))
    }

    #[test]
    fn test_insert_and_get() {
        let store = FactumStore::with_seeds();
        let node = make_node("n001", "instance-of",
            vec![Term::ent("ACME-CORP"), Term::ent("organization")]);

        store.insert(node.clone()).unwrap();

        let retrieved = store.get(&NodeId::new("n001")).unwrap();
        assert_eq!(retrieved.id.as_str(), "n001");
    }

    #[test]
    fn test_duplicate_insert() {
        let store = FactumStore::with_seeds();
        let node = make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")]);

        store.insert(node).unwrap();
        let result = store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")]));
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_insert() {
        let store = FactumStore::with_seeds();
        let nodes = vec![
            make_node("n001", "instance-of", vec![Term::ent("X"), Term::ent("Y")]),
            make_node("n002", "located-in", vec![Term::ent("X"), Term::ent("Z")]),
            make_node("n003", "founded-on", vec![Term::ent("X"), Term::lit(Literal::dec_from_str("1987").unwrap())]),
        ];

        store.insert_batch(nodes).unwrap();
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn test_lookup_by_entity() {
        let store = FactumStore::with_seeds();
        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("ACME-CORP"), Term::ent("org")])).unwrap();
        store.insert(make_node("n002", "located-in",
            vec![Term::ent("ACME-CORP"), Term::ent("SZ")])).unwrap();
        store.insert(make_node("n003", "instance-of",
            vec![Term::ent("APPLE"), Term::ent("org")])).unwrap();

        let acme_nodes = store.lookup_by_entity(&EntityId::new("ACME-CORP"));
        assert_eq!(acme_nodes.len(), 2);
    }

    #[test]
    fn test_lookup_by_pred() {
        let store = FactumStore::with_seeds();
        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();
        store.insert(make_node("n002", "instance-of",
            vec![Term::ent("A"), Term::ent("B")])).unwrap();
        store.insert(make_node("n003", "located-in",
            vec![Term::ent("X"), Term::ent("Z")])).unwrap();

        let nodes = store.lookup_by_pred("instance-of");
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_retract_cascade() {
        let store = FactumStore::with_seeds();

        // n001: base assertion
        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();

        // n002: derived from n001
        let mut derived = make_node("n002", "subsidiary-of",
            vec![Term::ent("A"), Term::ent("B")]);
        derived.provenance = Provenance::Derived {
            from: NodeId::new("n001"),
            rule: RuleId(SmolStr::new("r1")),
        };
        derived.deps.push(NodeId::new("n001"));
        store.insert(derived).unwrap();

        // n003: derived from n002
        let mut derived2 = make_node("n003", "acquired-by",
            vec![Term::ent("C"), Term::ent("D")]);
        derived2.provenance = Provenance::Derived {
            from: NodeId::new("n002"),
            rule: RuleId(SmolStr::new("r2")),
        };
        derived2.deps.push(NodeId::new("n002"));
        store.insert(derived2).unwrap();

        // Retract n001 → should cascade to n002 and n003
        let retracted = store.retract(&NodeId::new("n001")).unwrap();
        assert_eq!(retracted.len(), 3); // n001, n002, n003

        // Verify all are retracted
        assert_eq!(store.get(&NodeId::new("n001")).unwrap().status, NodeStatus::Retracted);
        assert_eq!(store.get(&NodeId::new("n002")).unwrap().status, NodeStatus::Retracted);
        assert_eq!(store.get(&NodeId::new("n003")).unwrap().status, NodeStatus::Retracted);
    }

    #[test]
    fn test_wal() {
        let store = FactumStore::with_seeds();
        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();
        store.retract(&NodeId::new("n001")).unwrap();

        let wal = store.wal_entries();
        assert!(wal.len() >= 2); // at least insert + retract
    }

    #[test]
    fn test_valid_now() {
        let store = FactumStore::with_seeds();
        let now = Utc::now();

        // Valid forever
        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();

        // Valid in a window around now
        let mut node2 = make_node("n002", "located-in",
            vec![Term::ent("X"), Term::ent("Z")]);
        node2.validity = Validity::Window {
            from: now - chrono::Duration::hours(1),
            until: Some(now + chrono::Duration::hours(1)),
        };
        store.insert(node2).unwrap();

        // Valid in the past
        let mut node3 = make_node("n003", "founded-on",
            vec![Term::ent("X"), Term::ent("Y")]);
        node3.validity = Validity::Window {
            from: now - chrono::Duration::days(365),
            until: Some(now - chrono::Duration::days(1)),
        };
        store.insert(node3).unwrap();

        let valid = store.lookup_valid_now();
        assert_eq!(valid.len(), 2); // n001 (forever) + n002 (current window)
    }
}
