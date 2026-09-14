//! RocksDB storage backend — persistent LSM-tree storage.
//!
//! ## Column Families
//! | CF | Key | Value | Purpose |
//! |----|-----|-------|---------|
//! | `nodes` | NodeId string bytes | bincode(Node) | Main store |
//! | `by_entity` | EntityId + \x00 + NodeId | empty | Entity → Nodes index |
//! | `by_pred` | pred_name + \x00 + NodeId | empty | Predicate → Nodes index |
//! | `by_src` | src_key + \x00 + NodeId | empty | Source → Nodes index |
//! | `by_perm` | perm_u32_BE + \x00 + NodeId | empty | Permission → Nodes index |
//!
//! ## Atomicity
//! `batch_write` uses RocksDB's `WriteBatch` for atomic multi-key writes.
//! RocksDB's native WAL ensures durability across crashes.
//!
//! ## Index rebuild
//! `deps_rev` and `by_validity` are NOT stored in RocksDB — they are
//! rebuilt in-memory by `FactumStore::rebuild_indices()` on startup.

#![cfg(feature = "rocksdb")]

use std::sync::Arc;
use std::path::Path;
use rocksdb::{DB, Options, ColumnFamilyDescriptor, WriteBatch};
use factum_core::types::*;
use crate::storage::{StorageBackend, StorageError, WriteOp, cf};

/// RocksDB-backed storage.
pub struct RocksDBBackend {
    db: Arc<DB>,
}

impl RocksDBBackend {
    /// Open or create a RocksDB database at the given path.
    pub fn open(path: impl AsRef<Path>, _registry: Arc<factum_core::morphemes::MorphemeRegistry>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        // Define column families
        let cf_names = [cf::NODES, cf::BY_ENTITY, cf::BY_PRED, cf::BY_SRC, cf::BY_PERM];
        let cf_descriptors: Vec<ColumnFamilyDescriptor> = cf_names.iter()
            .map(|name| {
                let mut cf_opts = Options::default();
                cf_opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB memtable
                ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        let db = DB::open_cf_descriptors(&opts, path, cf_descriptors)
            .map_err(|e| StorageError::Io(e.to_string()))?;

        Ok(Self { db: Arc::new(db) })
    }

    fn cf_handle(&self, name: &str) -> Result<&rocksdb::ColumnFamily, StorageError> {
        self.db.cf_handle(name)
            .ok_or_else(|| StorageError::CfNotFound(name.to_string()))
    }

    fn serialize_node(node: &Node) -> Result<Vec<u8>, StorageError> {
        bincode::serialize(node)
            .map_err(|e| StorageError::Serialization(e.to_string()))
    }

    fn deserialize_node(bytes: &[u8]) -> Result<Node, StorageError> {
        bincode::deserialize(bytes)
            .map_err(|e| StorageError::Serialization(e.to_string()))
    }
}

impl StorageBackend for RocksDBBackend {
    fn get_node(&self, id: &NodeId) -> Result<Option<Arc<Node>>, StorageError> {
        let cf = self.cf_handle(cf::NODES)?;
        match self.db.get_cf(cf, id.as_str().as_bytes()) {
            Ok(Some(bytes)) => {
                let node = Self::deserialize_node(&bytes)?;
                Ok(Some(Arc::new(node)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(StorageError::Io(e.to_string())),
        }
    }

    fn put_node(&self, node: &Node) -> Result<(), StorageError> {
        let cf = self.cf_handle(cf::NODES)?;
        let bytes = Self::serialize_node(node)?;
        self.db.put_cf(cf, node.id.as_str().as_bytes(), bytes)
            .map_err(|e| StorageError::Io(e.to_string()))
    }

    fn delete_node(&self, id: &NodeId) -> Result<(), StorageError> {
        let cf = self.cf_handle(cf::NODES)?;
        self.db.delete_cf(cf, id.as_str().as_bytes())
            .map_err(|e| StorageError::Io(e.to_string()))
    }

    fn scan_index(&self, cf_name: &str, prefix: &[u8]) -> Result<Vec<NodeId>, StorageError> {
        let cf = self.cf_handle(cf_name)?;
        let search_prefix = crate::storage::encode_index_prefix(prefix);
        let iter = self.db.prefix_iterator_cf(cf, search_prefix.clone());

        let mut result = Vec::new();
        for item in iter {
            let (key, _value) = item.map_err(|e| StorageError::Io(e.to_string()))?;
            // Check if key still matches the prefix
            if !key.starts_with(&search_prefix) {
                break; // Iterator went past the prefix range
            }
            // Extract NodeId from the key (after the prefix + \x00)
            if let Some(node_id) = crate::storage::decode_node_id_from_index(&key) {
                result.push(node_id);
            }
        }
        Ok(result)
    }

    fn put_index(&self, cf_name: &str, key: &[u8], _node_id: &NodeId) -> Result<(), StorageError> {
        let cf = self.cf_handle(cf_name)?;
        // For index CFs, the key already contains the node_id suffix.
        // Value is empty (we only care about key existence).
        self.db.put_cf(cf, key, b"")
            .map_err(|e| StorageError::Io(e.to_string()))
    }

    fn delete_index(&self, cf_name: &str, key: &[u8], _node_id: &NodeId) -> Result<(), StorageError> {
        let cf = self.cf_handle(cf_name)?;
        // The key already uniquely identifies the index entry.
        self.db.delete_cf(cf, key)
            .map_err(|e| StorageError::Io(e.to_string()))
    }

    fn batch_write(&self, ops: Vec<WriteOp>) -> Result<(), StorageError> {
        let mut batch = WriteBatch::default();

        for op in ops {
            match op {
                WriteOp::PutNode(node) => {
                    let cf = self.cf_handle(cf::NODES)?;
                    let bytes = Self::serialize_node(&node)?;
                    batch.put_cf(cf, node.id.as_str().as_bytes(), bytes);
                }
                WriteOp::DeleteNode(id) => {
                    let cf = self.cf_handle(cf::NODES)?;
                    batch.delete_cf(cf, id.as_str().as_bytes());
                }
                WriteOp::PutIndex { cf: cf_name, key, .. } => {
                    let cf = self.cf_handle(cf_name)?;
                    batch.put_cf(cf, key, b"");
                }
                WriteOp::DeleteIndex { cf: cf_name, key, .. } => {
                    let cf = self.cf_handle(cf_name)?;
                    batch.delete_cf(cf, key);
                }
            }
        }

        self.db.write(batch)
            .map_err(|e| StorageError::Io(e.to_string()))
    }

    fn len(&self) -> Result<usize, StorageError> {
        let cf = self.cf_handle(cf::NODES)?;
        let mut count = 0;
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        for item in iter {
            item.map_err(|e| StorageError::Io(e.to_string()))?;
            count += 1;
        }
        Ok(count)
    }

    fn iter_nodes(&self) -> Result<Vec<Arc<Node>>, StorageError> {
        let cf = self.cf_handle(cf::NODES)?;
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        let mut nodes = Vec::new();
        for item in iter {
            let (_key, value) = item.map_err(|e| StorageError::Io(e.to_string()))?;
            let node = Self::deserialize_node(&value)?;
            nodes.push(Arc::new(node));
        }
        Ok(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use factum_core::morphemes::MorphemeRegistry;
    use smol_str::SmolStr;

    fn make_node(id: &str, pred_name: &str, args: Vec<Term>) -> Node {
        Node::new(id, Predicate::new(pred_name).with_args(args))
    }

    fn make_backend() -> (RocksDBBackend, TempDir) {
        let dir = TempDir::new().unwrap();
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let backend = RocksDBBackend::open(dir.path(), registry).unwrap();
        (backend, dir)
    }

    #[test]
    fn test_rocksdb_insert_and_get() {
        let (backend, _dir) = make_backend();
        let node = make_node("n001", "instance-of",
            vec![Term::ent("ACME-CORP"), Term::ent("organization")]);

        backend.put_node(&node).unwrap();

        let retrieved = backend.get_node(&NodeId::new("n001")).unwrap().unwrap();
        assert_eq!(retrieved.id.as_str(), "n001");
        assert_eq!(retrieved.predicate.args.len(), 2);
    }

    #[test]
    fn test_rocksdb_get_nonexistent() {
        let (backend, _dir) = make_backend();
        let result = backend.get_node(&NodeId::new("nonexistent")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_rocksdb_delete_node() {
        let (backend, _dir) = make_backend();
        let node = make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")]);

        backend.put_node(&node).unwrap();
        assert!(backend.get_node(&NodeId::new("n001")).unwrap().is_some());

        backend.delete_node(&NodeId::new("n001")).unwrap();
        assert!(backend.get_node(&NodeId::new("n001")).unwrap().is_none());
    }

    #[test]
    fn test_rocksdb_len() {
        let (backend, _dir) = make_backend();
        assert_eq!(backend.len().unwrap(), 0);

        backend.put_node(&make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();
        backend.put_node(&make_node("n002", "located-in",
            vec![Term::ent("X"), Term::ent("Z")])).unwrap();

        assert_eq!(backend.len().unwrap(), 2);
    }

    #[test]
    fn test_rocksdb_iter_nodes() {
        let (backend, _dir) = make_backend();
        backend.put_node(&make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")])).unwrap();
        backend.put_node(&make_node("n002", "located-in",
            vec![Term::ent("X"), Term::ent("Z")])).unwrap();

        let nodes = backend.iter_nodes().unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_rocksdb_batch_write() {
        let (backend, _dir) = make_backend();
        let node1 = make_node("n001", "instance-of",
            vec![Term::ent("ACME-CORP"), Term::ent("organization")]);
        let node2 = make_node("n002", "located-in",
            vec![Term::ent("ACME-CORP"), Term::ent("SZ")]);

        let ops = vec![
            WriteOp::PutNode(node1.clone()),
            WriteOp::PutNode(node2.clone()),
            WriteOp::PutIndex {
                cf: cf::BY_ENTITY,
                key: crate::storage::encode_index_key(b"ACME-CORP", &NodeId::new("n001")),
                node_id: NodeId::new("n001"),
            },
            WriteOp::PutIndex {
                cf: cf::BY_ENTITY,
                key: crate::storage::encode_index_key(b"ACME-CORP", &NodeId::new("n002")),
                node_id: NodeId::new("n002"),
            },
        ];

        backend.batch_write(ops).unwrap();
        assert_eq!(backend.len().unwrap(), 2);

        // Check index scan
        let ids = backend.scan_index(cf::BY_ENTITY, b"ACME-CORP").unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_rocksdb_index_scan() {
        let (backend, _dir) = make_backend();

        // Add index entries
        backend.put_index(cf::BY_ENTITY,
            &crate::storage::encode_index_key(b"ACME-CORP", &NodeId::new("n001")),
            &NodeId::new("n001")).unwrap();
        backend.put_index(cf::BY_ENTITY,
            &crate::storage::encode_index_key(b"ACME-CORP", &NodeId::new("n002")),
            &NodeId::new("n002")).unwrap();
        backend.put_index(cf::BY_ENTITY,
            &crate::storage::encode_index_key(b"APPLE", &NodeId::new("n003")),
            &NodeId::new("n003")).unwrap();

        let acme_ids = backend.scan_index(cf::BY_ENTITY, b"ACME-CORP").unwrap();
        assert_eq!(acme_ids.len(), 2);

        let apple_ids = backend.scan_index(cf::BY_ENTITY, b"APPLE").unwrap();
        assert_eq!(apple_ids.len(), 1);
    }

    #[test]
    fn test_rocksdb_index_delete() {
        let (backend, _dir) = make_backend();
        let key = crate::storage::encode_index_key(b"ACME-CORP", &NodeId::new("n001"));

        backend.put_index(cf::BY_ENTITY, &key, &NodeId::new("n001")).unwrap();
        assert_eq!(backend.scan_index(cf::BY_ENTITY, b"ACME-CORP").unwrap().len(), 1);

        backend.delete_index(cf::BY_ENTITY, &key, &NodeId::new("n001")).unwrap();
        assert_eq!(backend.scan_index(cf::BY_ENTITY, b"ACME-CORP").unwrap().len(), 0);
    }

    #[test]
    fn test_rocksdb_persistence() {
        let dir = TempDir::new().unwrap();
        let registry = Arc::new(MorphemeRegistry::with_seeds());

        // Write data
        {
            let backend = RocksDBBackend::open(dir.path(), registry.clone()).unwrap();
            backend.put_node(&make_node("n001", "instance-of",
                vec![Term::ent("ACME-CORP"), Term::ent("organization")])).unwrap();
            backend.put_node(&make_node("n002", "located-in",
                vec![Term::ent("ACME-CORP"), Term::ent("SZ")])).unwrap();
        }

        // Reopen and verify data persists
        {
            let backend = RocksDBBackend::open(dir.path(), registry).unwrap();
            assert_eq!(backend.len().unwrap(), 2);

            let node = backend.get_node(&NodeId::new("n001")).unwrap().unwrap();
            assert_eq!(node.id.as_str(), "n001");
            assert_eq!(node.predicate.args.len(), 2);
        }
    }

    #[test]
    fn test_rocksdb_overwrite_node() {
        let (backend, _dir) = make_backend();
        let node = make_node("n001", "instance-of",
            vec![Term::ent("X"), Term::ent("Y")]);

        backend.put_node(&node).unwrap();

        // Overwrite with different predicate
        let node2 = make_node("n001", "located-in",
            vec![Term::ent("X"), Term::ent("Z")]);
        backend.put_node(&node2).unwrap();

        let retrieved = backend.get_node(&NodeId::new("n001")).unwrap().unwrap();
        // Should have the new predicate
        let head_name = match &retrieved.predicate.head {
            PredicateHead::Name(n) => n.as_str(),
            _ => "",
        };
        assert_eq!(head_name, "located-in");
    }

    #[test]
    fn test_rocksdb_complex_node_serialization() {
        let (backend, _dir) = make_backend();

        // Node with all fields populated
        let node = Node::new("n001",
            Predicate::new("shareholder-major")
                .with_args(vec![
                    Term::ent("ACME-CORP"),
                    Term::ent("FOUNDER-1"),
                    Term::lit(Literal::dec_from_str("0.73").unwrap()),
                ]))
            .with_confidence(Confidence(0.85))
            .with_authority(Authority(0.8))
            .with_permissions(PermissionTag::CONFIDENTIAL)
            .with_provenance(Provenance::Extracted {
                doc: DocId::new("doc002"),
                span: Span { start: 100, end: 200 },
                model: ModelRef {
                    name: SmolStr::new("gpt-4"),
                    version: SmolStr::new("2024-06"),
                },
            });

        backend.put_node(&node).unwrap();
        let retrieved = backend.get_node(&NodeId::new("n001")).unwrap().unwrap();

        assert_eq!(retrieved.id, node.id);
        assert_eq!(retrieved.confidence, node.confidence);
        assert_eq!(retrieved.authority, node.authority);
        assert_eq!(retrieved.permissions, node.permissions);
        assert_eq!(retrieved.provenance, node.provenance);
        assert_eq!(retrieved.predicate.args.len(), 3);
    }

    #[test]
    fn test_rocksdb_validity_window_node() {
        let (backend, _dir) = make_backend();
        let now = chrono::Utc::now();

        let mut node = make_node("n001", "located-in",
            vec![Term::ent("X"), Term::ent("Z")]);
        node.validity = Validity::Window {
            from: now - chrono::Duration::hours(1),
            until: Some(now + chrono::Duration::hours(1)),
        };

        backend.put_node(&node).unwrap();
        let retrieved = backend.get_node(&NodeId::new("n001")).unwrap().unwrap();

        match retrieved.validity {
            Validity::Window { from, until } => {
                assert_eq!(from, now - chrono::Duration::hours(1));
                assert!(until.is_some());
            }
            Validity::Forever => panic!("expected Window, got Forever"),
        }
    }
}
