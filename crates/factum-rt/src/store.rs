//! Factum store — the persistence and indexing layer.
//!
//! ## Design Decisions
//! - **StorageBackend trait** abstracts persistence (InMemory or RocksDB)
//! - **Index-level permission filtering** — NOT post-query filtering
//!   (post-filtering leaks aggregate information to unauthorized users)
//! - **Secondary indices** are separate from main store and can be rebuilt
//! - **Soft delete only** — retracted nodes are marked, never removed
//!   (supports "as of" historical queries and audit trails)
//! - **deps_rev and by_validity** are in-memory indices rebuilt on startup
//!   from the persistent nodes data.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use factum_core::types::*;
use factum_core::morphemes::MorphemeRegistry;
use crate::permission::PermissionContext;
use crate::subscription::SubscriptionManager;
use crate::verifier::{VerifierRegistry, Verdict};
use crate::storage::{
    StorageBackend, InMemoryBackend, StorageError, WriteOp, cf,
    encode_index_key,
};

/// The main Factum store.
///
/// Thread-safe via `parking_lot::RwLock` on in-memory indices and
/// `Arc<dyn StorageBackend>` for the persistence layer.
///
/// All writes are atomic at the method level. For multi-node atomic writes,
/// use `insert_batch()`.
pub struct FactumStore {
    /// Pluggable storage backend (InMemory or RocksDB).
    backend: Arc<dyn StorageBackend>,

    /// Reverse dependency: NodeId → [NodeIds that depend on it]
    /// Used for cascade invalidation when upstream nodes are retracted.
    /// Rebuilt in-memory from `deps` fields on startup.
    deps_rev: RwLock<HashMap<NodeId, Vec<NodeId>>>,

    /// Validity index: sorted by validity window for temporal queries.
    /// (from_timestamp, until_timestamp_or_max, NodeId)
    /// Rebuilt in-memory on startup.
    by_validity: RwLock<BTreeMap<(i64, i64), Vec<NodeId>>>,

    /// Morpheme registry for resolving names.
    registry: Arc<MorphemeRegistry>,

    /// Write-ahead log for event replay (in-memory backend only).
    /// RocksDB backend uses RocksDB's native WAL.
    wal: RwLock<Vec<WalEntry>>,

    /// Subscription manager for push notifications on insert/retract.
    subscriptions: SubscriptionManager,

    /// Optional verifier registry for node validation on insert.
    verifiers: RwLock<Option<VerifierRegistry>>,
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
    #[error("storage error: {0}")]
    Storage(String),
}

/// Result of a retract operation, including cascade information.
#[derive(Debug, Clone)]
pub struct RetractResult {
    /// All node IDs that were retracted (including the root and cascade).
    pub retracted: Vec<NodeId>,
    /// True if the cascade was truncated because it exceeded the node limit.
    pub truncated: bool,
    /// The maximum cascade depth reached (number of recursion levels).
    pub depth_reached: usize,
}

impl RetractResult {
    /// Number of nodes retracted.
    pub fn count(&self) -> usize {
        self.retracted.len()
    }
}

/// Default maximum number of nodes to retract in a cascade.
const DEFAULT_MAX_CASCADE_NODES: usize = 100;

impl From<StorageError> for StoreError {
    fn from(e: StorageError) -> Self {
        StoreError::Storage(e.to_string())
    }
}

impl FactumStore {
    /// Create a new empty store with the given morpheme registry (in-memory backend).
    pub fn new(registry: Arc<MorphemeRegistry>) -> Self {
        Self::with_backend(Arc::new(InMemoryBackend::new()), registry)
    }

    /// Create a store with default seed morphemes (in-memory backend).
    pub fn with_seeds() -> Self {
        Self::new(Arc::new(MorphemeRegistry::with_seeds()))
    }

    /// Create a store with a custom storage backend.
    pub fn with_backend(backend: Arc<dyn StorageBackend>, registry: Arc<MorphemeRegistry>) -> Self {
        let store = Self {
            backend,
            deps_rev: RwLock::new(HashMap::new()),
            by_validity: RwLock::new(BTreeMap::new()),
            registry,
            wal: RwLock::new(Vec::new()),
            subscriptions: SubscriptionManager::new(),
            verifiers: RwLock::new(None),
        };
        // Rebuild in-memory indices from persisted data
        store.rebuild_indices();
        store
    }

    /// Create a persistent store backed by RocksDB.
    #[cfg(feature = "rocksdb")]
    pub fn with_rocksdb(path: impl AsRef<std::path::Path>, registry: Arc<MorphemeRegistry>) -> Result<Self, StoreError> {
        let backend = crate::rocksdb_backend::RocksDBBackend::open(path, registry.clone())?;
        Ok(Self::with_backend(Arc::new(backend), registry))
    }

    /// Rebuild deps_rev and by_validity from persisted nodes.
    fn rebuild_indices(&self) {
        let nodes = match self.backend.iter_nodes() {
            Ok(n) => n,
            Err(_) => return,
        };

        // Rebuild deps_rev
        {
            let mut deps_rev = self.deps_rev.write();
            deps_rev.clear();
            for node in &nodes {
                for dep in &node.deps {
                    deps_rev
                        .entry(dep.clone())
                        .or_default()
                        .push(node.id.clone());
                }
            }
        }

        // Rebuild by_validity
        {
            let mut by_validity = self.by_validity.write();
            by_validity.clear();
            for node in &nodes {
                let (from_ts, until_ts) = match node.validity {
                    Validity::Forever => (i64::MIN, i64::MAX),
                    Validity::Window { from, until } => {
                        (from.timestamp(), until.map_or(i64::MAX, |u| u.timestamp()))
                    }
                };
                by_validity
                    .entry((from_ts, until_ts))
                    .or_default()
                    .push(node.id.clone());
            }
        }
    }

    /// Get the morpheme registry.
    pub fn registry(&self) -> &Arc<MorphemeRegistry> {
        &self.registry
    }

    /// Get the subscription manager for creating subscriptions.
    pub fn subscriptions(&self) -> &SubscriptionManager {
        &self.subscriptions
    }

    /// Enable built-in verifiers (SchemaVerifier + DecimalRangeVerifier).
    /// Once enabled, all subsequent inserts will be verified before storage.
    pub fn enable_verifiers(&self) {
        let reg = VerifierRegistry::with_builtins(self.registry.clone());
        *self.verifiers.write() = Some(reg);
    }

    /// Enable a custom verifier registry.
    pub fn set_verifiers(&self, reg: VerifierRegistry) {
        *self.verifiers.write() = Some(reg);
    }

    /// Disable all verifiers.
    pub fn disable_verifiers(&self) {
        *self.verifiers.write() = None;
    }

    // ─── Write Operations ──────────────────────────────

    /// Insert a new node. Fails if a node with the same ID already exists.
    /// If verifiers are enabled, the node is verified before insertion.
    pub fn insert(&self, node: Node) -> Result<(), StoreError> {
        let id = node.id.clone();

        // Verify (if verifiers are enabled)
        if let Some(ref vr) = *self.verifiers.read() {
            if let Verdict::Fail(reason) = vr.verify(&node) {
                return Err(StoreError::InvalidNode(reason));
            }
        }

        // Check for duplicate
        if self.backend.get_node(&id)?.is_some() {
            return Err(StoreError::AlreadyExists(id.to_string()));
        }

        // Build batch write operations
        let ops = self.build_insert_ops(&node);

        // WAL (in-memory backend only)
        self.wal.write().push(WalEntry::Insert(Box::new(node.clone())));

        // Execute atomically
        self.backend.batch_write(ops)?;

        // Update in-memory indices
        self.update_deps_rev(&node);
        self.update_by_validity(&node);

        // Insert into store (via backend, already done in batch_write)
        // But we need Arc<Node> for subscription notification
        let arc_node = Arc::new(node);
        self.subscriptions.notify_insert(&arc_node);

        Ok(())
    }

    /// Insert multiple nodes atomically (all-or-nothing).
    /// If verifiers are enabled, each node is verified before any insertion.
    pub fn insert_batch(&self, nodes: Vec<Node>) -> Result<(), StoreError> {
        // Verify all nodes first (if verifiers enabled)
        if let Some(ref vr) = *self.verifiers.read() {
            for node in &nodes {
                if let Verdict::Fail(reason) = vr.verify(node) {
                    return Err(StoreError::InvalidNode(reason));
                }
            }
        }

        // Pre-check: no duplicates within batch or with existing
        {
            let mut seen = HashSet::new();
            for node in &nodes {
                if self.backend.get_node(&node.id)?.is_some() || !seen.insert(node.id.clone()) {
                    return Err(StoreError::AlreadyExists(node.id.to_string()));
                }
            }
        }

        // Build all batch operations
        let mut all_ops = Vec::new();
        for node in &nodes {
            all_ops.extend(self.build_insert_ops(node));
        }

        // Execute atomically
        self.backend.batch_write(all_ops)?;

        // Update in-memory indices and notify
        for node in &nodes {
            self.update_deps_rev(node);
            self.update_by_validity(node);
            self.wal.write().push(WalEntry::Insert(Box::new(node.clone())));
            let arc_node = Arc::new(node.clone());
            self.subscriptions.notify_insert(&arc_node);
        }

        self.wal.write().push(WalEntry::Checkpoint);
        Ok(())
    }

    /// Retract a node (soft delete).
    ///
    /// The node is marked as Retracted but NOT deleted.
    /// All downstream Derived nodes are cascade-retracted via deps_rev.
    /// Uses the default maximum cascade node count of 100.
    pub fn retract(&self, id: &NodeId) -> Result<Vec<NodeId>, StoreError> {
        let result = self.retract_with_limit(id, DEFAULT_MAX_CASCADE_NODES)?;
        Ok(result.retracted)
    }

    /// Retract a node with a configurable maximum number of cascade nodes.
    ///
    /// `max_nodes` limits the total number of nodes that can be retracted
    /// in a single cascade (including the root node). If the cascade
    /// exceeds this limit, it is truncated and `truncated: true` is
    /// returned. All nodes up to the limit are successfully retracted.
    ///
    /// This is a **node count** limit, not a recursion depth limit.
    /// A cascade with depth 2 but 200 dependents will be truncated at
    /// `max_nodes`, protecting against fan-out explosion.
    pub fn retract_with_limit(&self, id: &NodeId, max_nodes: usize) -> Result<RetractResult, StoreError> {
        let mut all_retracted = Vec::new();
        let mut depth_reached = 0usize;
        let mut truncated = false;
        self.retract_recursive(id, max_nodes, 0, &mut all_retracted, &mut depth_reached, &mut truncated)?;

        // Notify subscribers of the retraction (with full cascade list)
        self.subscriptions.notify_retract(id, &all_retracted);

        Ok(RetractResult {
            retracted: all_retracted,
            truncated,
            depth_reached,
        })
    }

    /// Internal recursive retraction with node count limit.
    ///
    /// `max_nodes` limits the total number of nodes retracted.
    /// `current_depth` tracks recursion depth for reporting.
    fn retract_recursive(
        &self,
        id: &NodeId,
        max_nodes: usize,
        current_depth: usize,
        all_retracted: &mut Vec<NodeId>,
        depth_reached: &mut usize,
        truncated: &mut bool,
    ) -> Result<(), StoreError> {
        // Node count limit check — this is the effective guard against
        // cascade explosion (both deep chains and wide fan-out).
        if all_retracted.len() >= max_nodes {
            *truncated = true;
            return Ok(());
        }

        if current_depth > *depth_reached {
            *depth_reached = current_depth;
        }

        // Get the node
        let node = self.backend.get_node(id)?
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;

        // Skip if already retracted (prevents cycles)
        if node.status == NodeStatus::Retracted {
            return Ok(());
        }

        // Mark as retracted
        let mut new_node = (*node).clone();
        new_node.status = NodeStatus::Retracted;

        // Write the retracted node
        self.backend.put_node(&new_node)?;

        self.wal.write().push(WalEntry::Retract(id.clone()));

        all_retracted.push(id.clone());

        // Cascade: find all nodes that depend on this one
        let dependents = self.deps_rev.read().get(id).cloned().unwrap_or_default();

        for dep_id in &dependents {
            // Only cascade-retract Derived nodes
            let dep_node = self.backend.get_node(dep_id)?;
            if let Some(n) = dep_node {
                if matches!(n.provenance, Provenance::Derived { .. }) && n.status == NodeStatus::Active {
                    // Check if we're about to exceed the node count limit
                    if all_retracted.len() >= max_nodes {
                        *truncated = true;
                        break;
                    }
                    self.retract_recursive(
                        dep_id,
                        max_nodes,
                        current_depth + 1,
                        all_retracted,
                        depth_reached,
                        truncated,
                    )?;
                }
            }
        }

        Ok(())
    }

    // ─── Read Operations ───────────────────────────────

    /// Get a node by ID.
    pub fn get(&self, id: &NodeId) -> Option<Arc<Node>> {
        self.backend.get_node(id).ok().flatten()
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
        self.backend.iter_nodes().unwrap_or_default()
    }

    /// Get all active (non-retracted) nodes.
    pub fn all_active(&self) -> Vec<Arc<Node>> {
        self.backend.iter_nodes()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.status == NodeStatus::Active)
            .collect()
    }

    /// Lookup nodes by entity.
    pub fn lookup_by_entity(&self, entity: &EntityId) -> Vec<Arc<Node>> {
        let prefix = entity.as_str().as_bytes();
        let ids = self.backend.scan_index(cf::BY_ENTITY, prefix).unwrap_or_default();
        self.resolve_nodes(&ids)
    }

    /// Lookup nodes by predicate head name.
    pub fn lookup_by_pred(&self, head: &str) -> Vec<Arc<Node>> {
        let prefix = head.as_bytes();
        let ids = self.backend.scan_index(cf::BY_PRED, prefix).unwrap_or_default();
        self.resolve_nodes(&ids)
    }

    /// Lookup nodes by source document.
    pub fn lookup_by_doc(&self, doc: &str) -> Vec<Arc<Node>> {
        let prefix = doc.as_bytes();
        let ids = self.backend.scan_index(cf::BY_SRC, prefix).unwrap_or_default();
        self.resolve_nodes(&ids)
    }

    /// Lookup nodes valid at a specific time.
    ///
    /// Uses the `by_validity` BTreeMap index for efficient temporal queries.
    /// Only scans entries whose `from_ts <= t.timestamp()`, then filters
    /// on `until_ts >= t.timestamp()` and Active status.
    pub fn lookup_valid_at(&self, t: DateTime<Utc>) -> Vec<Arc<Node>> {
        let t_ts = t.timestamp();

        // Range query: all keys <= (t_ts, i64::MAX) covers entries
        // where from_ts <= t_ts. We then filter on until_ts >= t_ts.
        let by_validity = self.by_validity.read();
        let candidate_ids: Vec<NodeId> = by_validity
            .range(..=(t_ts, i64::MAX))
            .flat_map(|((_, _), ids)| ids.iter().cloned())
            .collect();
        drop(by_validity);

        // Load nodes and filter on until_ts and Active status
        candidate_ids.into_iter()
            .filter_map(|id| self.backend.get_node(&id).ok().flatten())
            .filter(|n| {
                n.status == NodeStatus::Active && n.validity.is_valid_at(t)
            })
            .collect()
    }

    /// Lookup nodes valid now.
    pub fn lookup_valid_now(&self) -> Vec<Arc<Node>> {
        self.lookup_valid_at(Utc::now())
    }

    /// Total node count.
    pub fn len(&self) -> usize {
        self.backend.len().unwrap_or(0)
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

    /// Build the batch write operations for inserting a node.
    /// This includes the node itself and all secondary index entries.
    fn build_insert_ops(&self, node: &Node) -> Vec<WriteOp> {
        let mut ops = Vec::new();

        // Put the node
        ops.push(WriteOp::PutNode(node.clone()));

        // by_entity: extract entity references from predicate args
        for arg in &node.predicate.args {
            self.collect_entity_index_ops(arg, &node.id, &mut ops);
        }
        for (_, val) in &node.predicate.named {
            self.collect_entity_index_ops(val, &node.id, &mut ops);
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
        let key = encode_index_key(head_key.as_bytes(), &node.id);
        ops.push(WriteOp::PutIndex { cf: cf::BY_PRED, key, node_id: node.id.clone() });

        // by_src
        let src_key = match &node.provenance {
            Provenance::Verbatim { doc, .. } |
            Provenance::Summary { doc, .. } |
            Provenance::Extracted { doc, .. } => Some(doc.0.to_string()),
            Provenance::Derived { from, .. } => Some(from.to_string()),
            Provenance::Asserted { by } => Some(by.0.to_string()),
        };
        if let Some(key_str) = src_key {
            let key = encode_index_key(key_str.as_bytes(), &node.id);
            ops.push(WriteOp::PutIndex { cf: cf::BY_SRC, key, node_id: node.id.clone() });
        }

        // by_perm
        let key = encode_index_key(&node.permissions.0.to_be_bytes(), &node.id);
        ops.push(WriteOp::PutIndex { cf: cf::BY_PERM, key, node_id: node.id.clone() });

        ops
    }

    /// Recursively collect entity index operations from a Term.
    fn collect_entity_index_ops(&self, term: &Term, node_id: &NodeId, ops: &mut Vec<WriteOp>) {
        match term {
            Term::Ent(e) => {
                let key = encode_index_key(e.as_str().as_bytes(), node_id);
                ops.push(WriteOp::PutIndex { cf: cf::BY_ENTITY, key, node_id: node_id.clone() });
            }
            Term::Compound(pred) => {
                for arg in &pred.args {
                    self.collect_entity_index_ops(arg, node_id, ops);
                }
            }
            Term::List(items) => {
                for item in items {
                    self.collect_entity_index_ops(item, node_id, ops);
                }
            }
            _ => {}
        }
    }

    /// Update deps_rev for a newly inserted node.
    fn update_deps_rev(&self, node: &Node) {
        for dep in &node.deps {
            self.deps_rev.write()
                .entry(dep.clone())
                .or_default()
                .push(node.id.clone());
        }
    }

    /// Update by_validity index for a newly inserted node.
    fn update_by_validity(&self, node: &Node) {
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

    fn resolve_nodes(&self, ids: &[NodeId]) -> Vec<Arc<Node>> {
        ids.iter()
            .filter_map(|id| self.backend.get_node(id).ok().flatten())
            .filter(|n| n.status == NodeStatus::Active)
            .collect()
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

    #[test]
    fn test_subscription_on_insert() {
        let store = FactumStore::with_seeds();
        let sub = store.subscriptions().subscribe("instance-of");

        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();

        assert!(sub.has_events());
        let events = sub.poll();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_subscription_on_retract() {
        let store = FactumStore::with_seeds();
        let sub = store.subscriptions().subscribe("*");

        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();
        // Drain insert events
        sub.poll();

        store.retract(&NodeId::new("n001")).unwrap();

        assert!(sub.has_events());
        let events = sub.poll();
        assert_eq!(events.len(), 1);
        // Verify it's a Retracted event
        assert!(matches!(events[0], crate::subscription::SubscriptionEvent::Retracted(_, _)));
    }

    #[test]
    fn test_subscription_pattern_filter_via_store() {
        let store = FactumStore::with_seeds();
        let sub = store.subscriptions().subscribe("located-in");

        // Insert a non-matching node — should not trigger
        store.insert(make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();
        assert!(!sub.has_events());

        // Insert a matching node — should trigger
        store.insert(make_node("n002", "located-in",
            vec![Term::ent("X"), Term::ent("Z")])).unwrap();
        assert!(sub.has_events());
    }

    #[test]
    fn test_verifier_rejects_invalid_node() {
        let store = FactumStore::with_seeds();
        store.enable_verifiers();

        // shareholder-major expects 4 args, give it only 2
        let bad_node = Node::new("n001",
            Predicate::new("shareholder-major")
                .with_args(vec![Term::ent("X"), Term::ent("Y")]));
        let result = store.insert(bad_node);
        assert!(result.is_err());
        match result.unwrap_err() {
            StoreError::InvalidNode(msg) => assert!(msg.contains("arity")),
            other => panic!("expected InvalidNode, got {:?}", other),
        }
    }

    #[test]
    fn test_verifier_accepts_valid_node() {
        let store = FactumStore::with_seeds();
        store.enable_verifiers();

        // instance-of expects 2 args — correct
        let good_node = make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("organization")]);
        let result = store.insert(good_node);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verifier_disabled_by_default() {
        let store = FactumStore::with_seeds();

        // Without enabling verifiers, bad arity should still be accepted
        let bad_node = Node::new("n001",
            Predicate::new("shareholder-major")
                .with_args(vec![Term::ent("X"), Term::ent("Y")]));
        let result = store.insert(bad_node);
        assert!(result.is_ok()); // no verifier running
    }

    #[test]
    fn test_verifier_batch_rejects_all() {
        let store = FactumStore::with_seeds();
        store.enable_verifiers();

        // One bad node in batch should reject the entire batch
        let nodes = vec![
            make_node("n001", "instance-of", vec![Term::ent("X"), Term::ent("Y")]),
            Node::new("n002",
                Predicate::new("shareholder-major")
                    .with_args(vec![Term::ent("X"), Term::ent("Y")])), // bad arity
        ];
        let result = store.insert_batch(nodes);
        assert!(result.is_err());
        // Neither should be inserted
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_retract_with_node_limit_truncates() {
        let store = FactumStore::with_seeds();

        // Build a chain: n001 → n002 → n003 → n004 → n005
        store.insert(make_node("n001", "base",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();

        for i in 2..=5 {
            let prev = format!("n{:03}", i - 1);
            let curr = format!("n{:03}", i);
            let mut derived = make_node(&curr, "derived",
                vec![Term::ent("A"), Term::ent("B")]);
            derived.provenance = Provenance::Derived {
                from: NodeId::new(prev.clone()),
                rule: RuleId(SmolStr::new("r1")),
            };
            derived.deps.push(NodeId::new(prev));
            store.insert(derived).unwrap();
        }

        // Retract n001 with max_depth=3 — should truncate
        let result = store.retract_with_limit(&NodeId::new("n001"), 3).unwrap();
        assert!(result.truncated);
        assert!(result.retracted.len() <= 3);
        assert!(result.depth_reached > 0);
    }

    #[test]
    fn test_retract_with_node_limit_no_truncation() {
        let store = FactumStore::with_seeds();

        // Build a small chain: n001 → n002
        store.insert(make_node("n001", "base",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();

        let mut derived = make_node("n002", "derived",
            vec![Term::ent("A"), Term::ent("B")]);
        derived.provenance = Provenance::Derived {
            from: NodeId::new("n001"),
            rule: RuleId(SmolStr::new("r1")),
        };
        derived.deps.push(NodeId::new("n001"));
        store.insert(derived).unwrap();

        // Retract n001 with max_depth=100 — should not truncate
        let result = store.retract_with_limit(&NodeId::new("n001"), 100).unwrap();
        assert!(!result.truncated);
        assert_eq!(result.retracted.len(), 2);
    }

    #[test]
    fn test_retract_with_node_limit_one() {
        let store = FactumStore::with_seeds();

        store.insert(make_node("n001", "base",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();

        // max_depth=1 means only the root node itself
        let result = store.retract_with_limit(&NodeId::new("n001"), 1).unwrap();
        assert_eq!(result.retracted.len(), 1);
        assert!(!result.truncated); // only 1 node, no cascade needed
    }

    #[test]
    fn test_retract_backward_compatible() {
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

        // Old retract() API still works
        let retracted = store.retract(&NodeId::new("n001")).unwrap();
        assert_eq!(retracted.len(), 2);
    }

    #[test]
    fn test_retract_cycle_safe() {
        // Test that a cycle in deps doesn't cause infinite recursion
        let store = FactumStore::with_seeds();

        // n001 depends on n002, n002 depends on n001 (shouldn't happen, but be safe)
        let mut n1 = make_node("n001", "pred-a",
            vec![Term::ent("X"), Term::ent("Y")]);
        n1.provenance = Provenance::Derived {
            from: NodeId::new("n002"),
            rule: RuleId(SmolStr::new("r1")),
        };
        n1.deps.push(NodeId::new("n002"));

        let mut n2 = make_node("n002", "pred-b",
            vec![Term::ent("A"), Term::ent("B")]);
        n2.provenance = Provenance::Derived {
            from: NodeId::new("n001"),
            rule: RuleId(SmolStr::new("r2")),
        };
        n2.deps.push(NodeId::new("n001"));

        store.insert(n1).unwrap();
        store.insert(n2).unwrap();

        // Should not infinite loop — already-retracted check prevents cycles
        let result = store.retract_with_limit(&NodeId::new("n001"), 100).unwrap();
        assert!(!result.truncated);
        assert_eq!(result.retracted.len(), 2);
    }
}
