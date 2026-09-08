//! # factum-rt
//!
//! Factum runtime: in-memory store, indexing, query, arbitration,
//! permission filtering, verification, and subscription.
//!
//! ## Architecture (3-layer)
//! ```text
//! ┌─ FactumStore ────────────────────────────────┐
//! │ Storage layer: HashMap (production: RocksDB)│ ← transactions
//! │   ├─ nodes/  main store, key = NodeId       │
//! │   ├─ index/  secondary indices              │
//! │   └─ graph/  adjacency (deps/reverse)       │
//! │ Logic layer: materialized views             │
//! │ Query layer: LogicEngine                    │
//! └─────────────────────────────────────────────┘
//! ```

pub mod store;
pub mod query;
pub mod arbitration;
pub mod permission;
pub mod verifier;
pub mod subscription;

pub use store::FactumStore;
pub use query::{Query, QueryOptions, ResultSet, QueryError};
pub use arbitration::{ConflictPolicy, ArbitrationResult};
pub use permission::{PermissionContext, PermissionError};
pub use verifier::{Verifier, Verdict, VerifierRegistry};
pub use subscription::{Subscription, SubscriptionEvent};
