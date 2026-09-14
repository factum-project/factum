//! Storage backend abstraction — allows swapping in-memory HashMap for RocksDB.
//!
//! ## Design
//! - `StorageBackend` trait abstracts the key-value operations needed by `FactumStore`.
//! - `InMemoryBackend` (default): HashMap-backed, identical to v0.1 behavior.
//! - `RocksDBBackend` (feature `rocksdb`): persistent LSM-tree with column families.
//! - `FactumStore` holds `Arc<dyn StorageBackend>` — dynamic dispatch, zero generic plumbing.
//!
//! ## Column Families (RocksDB)
//! | CF | Key | Value | Purpose |
//! |----|-----|-------|---------|
//! | `nodes` | NodeId bytes | bincode(Node) | Main store |
//! | `by_entity` | EntityId + \x00 + NodeId | empty | Entity → Nodes index |
//! | `by_pred` | pred_name + \x00 + NodeId | empty | Predicate → Nodes index |
//! | `by_src` | src_key + \x00 + NodeId | empty | Source → Nodes index |
//! | `by_perm` | perm_u32 + NodeId | empty | Permission → Nodes index |
//!
//! `deps_rev` and `by_validity` are rebuilt in-memory on startup from the `nodes` CF.

use std::sync::Arc;
use factum_core::types::*;

/// Write operation for atomic batch writes.
#[derive(Clone, Debug)]
pub enum WriteOp {
    /// Insert or overwrite a node.
    PutNode(Node),
    /// Delete a node by ID.
    DeleteNode(NodeId),
    /// Add an index entry: CF key → NodeId.
    PutIndex { cf: &'static str, key: Vec<u8>, node_id: NodeId },
    /// Remove an index entry: CF key → NodeId.
    DeleteIndex { cf: &'static str, key: Vec<u8>, node_id: NodeId },
}

/// Column family identifiers.
pub mod cf {
    pub const NODES: &str = "nodes";
    pub const BY_ENTITY: &str = "by_entity";
    pub const BY_PRED: &str = "by_pred";
    pub const BY_SRC: &str = "by_src";
    pub const BY_PERM: &str = "by_perm";
}

/// Storage backend trait — abstracts the persistence layer.
///
/// All methods return `Result<_, StorageError>` so that IO failures
/// (disk full, corruption, etc.) are propagated to the caller.
pub trait StorageBackend: Send + Sync {
    /// Get a node by ID.
    fn get_node(&self, id: &NodeId) -> Result<Option<Arc<Node>>, StorageError>;

    /// Insert or overwrite a node (no duplicate check — caller handles that).
    fn put_node(&self, node: &Node) -> Result<(), StorageError>;

    /// Delete a node by ID.
    fn delete_node(&self, id: &NodeId) -> Result<(), StorageError>;

    /// Scan index entries in a CF matching a prefix.
    /// Returns the NodeIds found.
    fn scan_index(&self, cf: &str, prefix: &[u8]) -> Result<Vec<NodeId>, StorageError>;

    /// Add an index entry: (CF, key) → NodeId.
    fn put_index(&self, cf: &str, key: &[u8], node_id: &NodeId) -> Result<(), StorageError>;

    /// Remove an index entry: (CF, key) → NodeId.
    fn delete_index(&self, cf: &str, key: &[u8], node_id: &NodeId) -> Result<(), StorageError>;

    /// Atomically execute a batch of write operations.
    /// All ops succeed or none do (all-or-nothing).
    fn batch_write(&self, ops: Vec<WriteOp>) -> Result<(), StorageError>;

    /// Total node count.
    fn len(&self) -> Result<usize, StorageError>;

    /// Is the store empty?
    fn is_empty(&self) -> Result<bool, StorageError> {
        self.len().map(|n| n == 0)
    }

    /// Iterate all nodes (for `all()` / `all_active()`).
    /// Warning: this loads all nodes into memory — admin/testing only.
    fn iter_nodes(&self) -> Result<Vec<Arc<Node>>, StorageError>;
}

/// Errors from storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("column family not found: {0}")]
    CfNotFound(String),
}

// ─── Index key encoding helpers ─────────────────────────────

/// Encode an index key: `prefix_bytes + \x00 + node_id_bytes`.
/// The \x00 separator is safe because Factum identifiers are ASCII-only.
pub fn encode_index_key(prefix: &[u8], node_id: &NodeId) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefix.len() + 1 + node_id.as_str().len());
    key.extend_from_slice(prefix);
    key.push(0);
    key.extend_from_slice(node_id.as_str().as_bytes());
    key
}

/// Encode the prefix for scanning (without the node_id suffix).
/// Returns `prefix_bytes + \x00`.
pub fn encode_index_prefix(prefix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefix.len() + 1);
    key.extend_from_slice(prefix);
    key.push(0);
    key
}

/// Decode a NodeId from the tail of an index key.
/// The key format is `prefix + \x00 + node_id`.
pub fn decode_node_id_from_index(key: &[u8]) -> Option<NodeId> {
    // Find the last \x00 separator
    // (we search from the end because node_id itself won't contain \x00)
    let sep_pos = key.iter().rposition(|&b| b == 0)?;
    let id_bytes = &key[sep_pos + 1..];
    let id_str = std::str::from_utf8(id_bytes).ok()?;
    Some(NodeId::new(id_str))
}

// ─── InMemoryBackend ────────────────────────────────────────

use std::collections::HashMap;
use parking_lot::RwLock;
use ahash::AHashMap;

/// Type alias for the index map: CF name → (key → Vec<NodeId>).
type IndexMap = HashMap<&'static str, HashMap<Vec<u8>, Vec<NodeId>>>;

/// In-memory storage backend using HashMap.
///
/// This is the default backend — identical behavior to pre-RocksDB FactumStore.
/// All operations are atomic at the method level via RwLock.
pub struct InMemoryBackend {
    /// Main store: NodeId → Arc<Node>
    nodes: RwLock<AHashMap<NodeId, Arc<Node>>>,
    /// Secondary index: key → Vec<NodeId>
    /// Key format depends on the CF (see encode_index_key).
    indices: RwLock<IndexMap>,
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryBackend {
    pub fn new() -> Self {
        let mut indices = HashMap::new();
        indices.insert(cf::BY_ENTITY, HashMap::new());
        indices.insert(cf::BY_PRED, HashMap::new());
        indices.insert(cf::BY_SRC, HashMap::new());
        indices.insert(cf::BY_PERM, HashMap::new());
        Self {
            nodes: RwLock::new(AHashMap::new()),
            indices: RwLock::new(indices),
        }
    }

    fn get_index_map(&self, cf: &str) -> Option<&'static str> {
        match cf {
            cf::BY_ENTITY => Some(cf::BY_ENTITY),
            cf::BY_PRED => Some(cf::BY_PRED),
            cf::BY_SRC => Some(cf::BY_SRC),
            cf::BY_PERM => Some(cf::BY_PERM),
            _ => None,
        }
    }
}

impl StorageBackend for InMemoryBackend {
    fn get_node(&self, id: &NodeId) -> Result<Option<Arc<Node>>, StorageError> {
        Ok(self.nodes.read().get(id).cloned())
    }

    fn put_node(&self, node: &Node) -> Result<(), StorageError> {
        self.nodes.write().insert(node.id.clone(), Arc::new(node.clone()));
        Ok(())
    }

    fn delete_node(&self, id: &NodeId) -> Result<(), StorageError> {
        self.nodes.write().remove(id);
        Ok(())
    }

    fn scan_index(&self, cf: &str, prefix: &[u8]) -> Result<Vec<NodeId>, StorageError> {
        let indices = self.indices.read();
        let cf_map = match indices.get(cf) {
            Some(m) => m,
            None => return Err(StorageError::CfNotFound(cf.to_string())),
        };
        let search_prefix = encode_index_prefix(prefix);
        let mut result = Vec::new();
        for (key, node_ids) in cf_map.iter() {
            if key.starts_with(&search_prefix) {
                result.extend(node_ids.iter().cloned());
            }
        }
        Ok(result)
    }

    fn put_index(&self, cf: &str, key: &[u8], node_id: &NodeId) -> Result<(), StorageError> {
        let cf_static = match self.get_index_map(cf) {
            Some(s) => s,
            None => return Err(StorageError::CfNotFound(cf.to_string())),
        };
        let mut indices = self.indices.write();
        let cf_map = indices.entry(cf_static).or_default();
        cf_map.entry(key.to_vec()).or_default().push(node_id.clone());
        Ok(())
    }

    fn delete_index(&self, cf: &str, key: &[u8], node_id: &NodeId) -> Result<(), StorageError> {
        let cf_static = match self.get_index_map(cf) {
            Some(s) => s,
            None => return Err(StorageError::CfNotFound(cf.to_string())),
        };
        let mut indices = self.indices.write();
        if let Some(cf_map) = indices.get_mut(cf_static) {
            if let Some(ids) = cf_map.get_mut(key) {
                ids.retain(|id| id != node_id);
                if ids.is_empty() {
                    cf_map.remove(key);
                }
            }
        }
        Ok(())
    }

    fn batch_write(&self, ops: Vec<WriteOp>) -> Result<(), StorageError> {
        // InMemory backend: just execute sequentially.
        // True atomicity would require a transaction, but HashMap ops don't fail.
        for op in ops {
            match op {
                WriteOp::PutNode(node) => { self.put_node(&node)?; }
                WriteOp::DeleteNode(id) => { self.delete_node(&id)?; }
                WriteOp::PutIndex { cf, key, node_id } => { self.put_index(cf, &key, &node_id)?; }
                WriteOp::DeleteIndex { cf, key, node_id } => { self.delete_index(cf, &key, &node_id)?; }
            }
        }
        Ok(())
    }

    fn len(&self) -> Result<usize, StorageError> {
        Ok(self.nodes.read().len())
    }

    fn iter_nodes(&self) -> Result<Vec<Arc<Node>>, StorageError> {
        Ok(self.nodes.read().values().cloned().collect())
    }
}
