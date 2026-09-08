//! Subscription and invalidation propagation.
//!
//! ## Design
//! - `watch(pred_pattern, validity_window)` returns a stream of events
//! - When upstream nodes are retracted → cascade via `deps_rev`
//! - Events are pushed to subscribers
//! - WAL supports full replay for late joiners
//!
//! In v0.1, we implement a simple polling-based subscription.
//! Production will use async streams (tokio::broadcast).

use std::sync::Arc;
use std::collections::VecDeque;
use parking_lot::Mutex;
use smol_str::SmolStr;
use chrono::{DateTime, Utc};
use factum_core::types::*;

/// A subscription to node changes matching a pattern.
pub struct Subscription {
    /// Pattern to match (predicate head name).
    pub pattern: SmolStr,
    /// Only receive events for nodes valid in this window.
    pub validity_window: Option<(DateTime<Utc>, Option<DateTime<Utc>>)>,
    /// Buffered events (in v0.1, polling-based).
    events: Mutex<VecDeque<SubscriptionEvent>>,
    /// Subscription ID.
    pub id: u64,
}

/// Events that subscribers receive.
#[derive(Clone, Debug)]
pub enum SubscriptionEvent {
    /// A new node was inserted matching the pattern.
    Inserted(Arc<Node>),
    /// A node was retracted (directly or via cascade).
    Retracted(NodeId, Vec<NodeId>),
}

impl Subscription {
    pub fn new(id: u64, pattern: impl Into<SmolStr>) -> Self {
        Self {
            pattern: pattern.into(),
            validity_window: None,
            events: Mutex::new(VecDeque::new()),
            id,
        }
    }

    /// Poll for events. Returns events since last poll.
    pub fn poll(&self) -> Vec<SubscriptionEvent> {
        let mut events = self.events.lock();
        let result: Vec<_> = events.drain(..).collect();
        result
    }

    /// Check if there are pending events.
    pub fn has_events(&self) -> bool {
        !self.events.lock().is_empty()
    }

    /// Push an event (internal).
    pub(crate) fn push(&self, event: SubscriptionEvent) {
        self.events.lock().push_back(event);
    }

    /// Check if this subscription matches a node.
    pub fn matches(&self, node: &Node) -> bool {
        // Check pattern
        let head_name = match &node.predicate.head {
            PredicateHead::Name(n) => n.as_str(),
            PredicateHead::Id(_) => return false, // simplified
        };
        if self.pattern != head_name && self.pattern != "*" {
            return false;
        }

        // Check validity window
        if let Some((from, _until)) = &self.validity_window {
            if !node.validity.is_valid_at(*from) {
                // Simplified: just check `from`
                return false;
            }
        }

        true
    }
}

/// Subscription manager.
pub struct SubscriptionManager {
    subscriptions: Mutex<Vec<Arc<Subscription>>>,
    next_id: Mutex<u64>,
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionManager {
    pub fn new() -> Self {
        Self {
            subscriptions: Mutex::new(Vec::new()),
            next_id: Mutex::new(0),
        }
    }

    /// Create a new subscription.
    pub fn subscribe(&self, pattern: impl Into<SmolStr>) -> Arc<Subscription> {
        let mut id_lock = self.next_id.lock();
        let id = *id_lock;
        *id_lock += 1;
        drop(id_lock);

        let sub = Arc::new(Subscription::new(id, pattern));
        self.subscriptions.lock().push(sub.clone());
        sub
    }

    /// Notify all subscribers of a new node insertion.
    pub fn notify_insert(&self, node: &Arc<Node>) {
        let subs = self.subscriptions.lock();
        for sub in subs.iter() {
            if sub.matches(node) {
                sub.push(SubscriptionEvent::Inserted(node.clone()));
            }
        }
    }

    /// Notify all subscribers of a retraction.
    pub fn notify_retract(&self, id: &NodeId, cascade: &[NodeId]) {
        let subs = self.subscriptions.lock();
        for sub in subs.iter() {
            // Notify all subscribers for now (pattern matching on retraction
            // would require looking up the node, which may already be retracted)
            sub.push(SubscriptionEvent::Retracted(id.clone(), cascade.to_vec()));
        }
    }

    /// Unsubscribe.
    pub fn unsubscribe(&self, id: u64) {
        self.subscriptions.lock().retain(|s| s.id != id);
    }

    /// Number of active subscriptions.
    pub fn count(&self) -> usize {
        self.subscriptions.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_basic() {
        let mgr = SubscriptionManager::new();
        let sub = mgr.subscribe("instance-of");

        let node = Arc::new(Node::new("n001",
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("X"), Term::ent("org")])));

        mgr.notify_insert(&node);

        assert!(sub.has_events());
        let events = sub.poll();
        assert_eq!(events.len(), 1);
        match &events[0] {
            SubscriptionEvent::Inserted(n) => assert_eq!(n.id.as_str(), "n001"),
            _ => panic!("expected Inserted event"),
        }
    }

    #[test]
    fn test_subscription_pattern_filter() {
        let mgr = SubscriptionManager::new();
        let sub = mgr.subscribe("located-in");

        // Non-matching node
        let node1 = Arc::new(Node::new("n001",
            Predicate::new("instance-of")
                .with_args(vec![Term::ent("X"), Term::ent("org")])));
        mgr.notify_insert(&node1);
        assert!(!sub.has_events());

        // Matching node
        let node2 = Arc::new(Node::new("n002",
            Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("SZ")])));
        mgr.notify_insert(&node2);
        assert!(sub.has_events());
    }

    #[test]
    fn test_subscription_wildcard() {
        let mgr = SubscriptionManager::new();
        let sub = mgr.subscribe("*");

        let node = Arc::new(Node::new("n001",
            Predicate::new("anything")
                .with_args(vec![Term::ent("X")])));
        mgr.notify_insert(&node);
        assert!(sub.has_events());
    }

    #[test]
    fn test_subscription_retract() {
        let mgr = SubscriptionManager::new();
        let sub = mgr.subscribe("*");

        let id = NodeId::new("n001");
        let cascade = vec![NodeId::new("n002"), NodeId::new("n003")];
        mgr.notify_retract(&id, &cascade);

        let events = sub.poll();
        assert_eq!(events.len(), 1);
        match &events[0] {
            SubscriptionEvent::Retracted(id, cascade) => {
                assert_eq!(id.as_str(), "n001");
                assert_eq!(cascade.len(), 2);
            }
            _ => panic!("expected Retracted event"),
        }
    }
}
