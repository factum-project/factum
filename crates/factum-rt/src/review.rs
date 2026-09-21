//! Review queue — human approval workflow for agent knowledge governance.
//!
//! ## Design Principle
//!
//! The queue only holds things the system **refuses to self-decide**:
//! - Ambiguous arbitration (system cannot pick one answer)
//! - Band clipping without corroboration (confidence exceeds evidence)
//! - Cascade retraction truncated (knowledge base in unknown half-state)
//!
//! Events the system CAN safely self-decide (e.g. corroboration success)
//! are auto-resolved and logged, never enqueued.
//!
//! ## Routing Rules
//!
//! | Event | Route | Reason |
//! |-------|-------|--------|
//! | Arbitration Ambiguous | Enqueue (normal) | System refused to guess |
//! | Corroboration success | Auto-resolve + log | System can safely self-decide |
//! | Band clipping (no corroboration) | Enqueue (normal) | Claims certainty beyond evidence |
//! | Cascade truncated | Enqueue (high priority) | KB in unknown half-retracted state |

use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Priority level for review events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReviewPriority {
    /// Normal priority — system refused to decide, human should review.
    Normal,
    /// High priority — knowledge base may be in an inconsistent state.
    /// Examples: cascade retraction truncated.
    High,
}

impl ReviewEventType {
    /// String representation for JSON serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewEventType::AmbiguousArbitration => "AmbiguousArbitration",
            ReviewEventType::BandClippingExceeded => "BandClippingExceeded",
            ReviewEventType::CascadeTruncated => "CascadeTruncated",
        }
    }
}

impl ReviewPriority {
    /// String representation for JSON serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewPriority::Normal => "Normal",
            ReviewPriority::High => "High",
        }
    }
}

impl ReviewStatus {
    /// String representation for JSON serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewStatus::Pending => "Pending",
            ReviewStatus::Approved => "Approved",
            ReviewStatus::Rejected => "Rejected",
            ReviewStatus::AutoResolved => "AutoResolved",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewEventType {
    /// Arbitration returned Ambiguous — multiple matching nodes with
    /// conflicting values, system refused to pick one.
    AmbiguousArbitration,
    /// A node's confidence exceeds the provenance band upper bound
    /// and no corroboration (>= 2 independent principals) exists.
    BandClippingExceeded,
    /// Cascade retraction was truncated — the cascade exceeded the
    /// node limit, leaving the knowledge base in a partially-retracted
    /// state. Human must decide whether to continue retraction or
    /// restore.
    CascadeTruncated,
}

/// Status of a review event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewStatus {
    /// Waiting for human review.
    Pending,
    /// Human approved — the fact/operation is allowed to stand.
    Approved,
    /// Human rejected — the fact should be retracted (if applicable).
    Rejected,
    /// System auto-resolved (e.g. corroboration arrived after enqueue).
    AutoResolved,
}

/// A review event — something the system needs a human to decide.
#[derive(Clone, Debug)]
pub struct ReviewEvent {
    /// Unique sequential ID.
    pub id: u64,
    /// What triggered this event.
    pub event_type: ReviewEventType,
    /// Priority level.
    pub priority: ReviewPriority,
    /// Node ID(s) involved in this event.
    pub node_ids: Vec<String>,
    /// Human-readable explanation of why this was enqueued.
    pub reason: String,
    /// Timestamp (Unix epoch seconds).
    pub timestamp: i64,
    /// Current status.
    pub status: ReviewStatus,
}

impl ReviewEvent {
    /// Whether this event still needs human attention.
    pub fn is_pending(&self) -> bool {
        self.status == ReviewStatus::Pending
    }
}

/// Review queue — thread-safe collection of review events.
///
/// Events are stored in a deque. Pending events are returned first
/// (sorted by priority: High before Normal). Resolved events are
/// retained for audit trail.
pub struct ReviewQueue {
    events: RwLock<VecDeque<ReviewEvent>>,
    next_id: AtomicU64,
}

impl Default for ReviewQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl ReviewQueue {
    /// Create a new empty review queue.
    pub fn new() -> Self {
        Self {
            events: RwLock::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Enqueue a new review event.
    ///
    /// Returns the assigned event ID.
    pub fn enqueue(
        &self,
        event_type: ReviewEventType,
        priority: ReviewPriority,
        node_ids: Vec<String>,
        reason: String,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let timestamp = chrono::Utc::now().timestamp();
        let event = ReviewEvent {
            id,
            event_type,
            priority,
            node_ids,
            reason,
            timestamp,
            status: ReviewStatus::Pending,
        };

        let mut events = self.events.write();
        // Insert high-priority events before normal-priority pending events.
        if priority == ReviewPriority::High {
            // Find the first non-pending or normal-priority event.
            let pos = events.iter().position(|e| {
                !e.is_pending() || e.priority == ReviewPriority::Normal
            }).unwrap_or(events.len());
            events.insert(pos, event);
        } else {
            // Normal priority: append after all pending events.
            let pos = events.iter().position(|e| !e.is_pending()).unwrap_or(events.len());
            events.insert(pos, event);
        }

        id
    }

    /// List all pending events, sorted by priority (High first).
    pub fn list_pending(&self) -> Vec<ReviewEvent> {
        let events = self.events.read();
        let mut pending: Vec<ReviewEvent> = events.iter()
            .filter(|e| e.is_pending())
            .cloned()
            .collect();
        // Sort by priority (High first), then by ID (oldest first).
        pending.sort_by(|a, b| {
            b.priority.cmp(&a.priority)
                .then(a.id.cmp(&b.id))
        });
        pending
    }

    /// List all events (including resolved), newest first.
    pub fn list_all(&self) -> Vec<ReviewEvent> {
        let events = self.events.read();
        let mut all: Vec<ReviewEvent> = events.iter().rev().cloned().collect();
        all.sort_by_key(|a| std::cmp::Reverse(a.id));
        all
    }

    /// Get a specific event by ID.
    pub fn get(&self, id: u64) -> Option<ReviewEvent> {
        let events = self.events.read();
        events.iter().find(|e| e.id == id).cloned()
    }

    /// Mark an event as approved.
    pub fn approve(&self, id: u64) -> Result<(), String> {
        let mut events = self.events.write();
        let event = events.iter_mut().find(|e| e.id == id)
            .ok_or_else(|| format!("review event {} not found", id))?;
        if event.status != ReviewStatus::Pending {
            return Err(format!("event {} is already {:?}, cannot approve", id, event.status));
        }
        event.status = ReviewStatus::Approved;
        Ok(())
    }

    /// Mark an event as rejected.
    pub fn reject(&self, id: u64) -> Result<(), String> {
        let mut events = self.events.write();
        let event = events.iter_mut().find(|e| e.id == id)
            .ok_or_else(|| format!("review event {} not found", id))?;
        if event.status != ReviewStatus::Pending {
            return Err(format!("event {} is already {:?}, cannot reject", id, event.status));
        }
        event.status = ReviewStatus::Rejected;
        Ok(())
    }

    /// Mark an event as auto-resolved (e.g. corroboration arrived).
    pub fn auto_resolve(&self, id: u64) -> Result<(), String> {
        let mut events = self.events.write();
        let event = events.iter_mut().find(|e| e.id == id)
            .ok_or_else(|| format!("review event {} not found", id))?;
        event.status = ReviewStatus::AutoResolved;
        Ok(())
    }

    /// Count of pending events.
    pub fn pending_count(&self) -> usize {
        self.events.read().iter().filter(|e| e.is_pending()).count()
    }

    /// Count of all events (including resolved).
    pub fn total_count(&self) -> usize {
        self.events.read().len()
    }
}

/// Shared review queue type.
pub type SharedReviewQueue = Arc<ReviewQueue>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enqueue_and_list_pending() {
        let queue = ReviewQueue::new();
        let id1 = queue.enqueue(
            ReviewEventType::AmbiguousArbitration,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "conflict between n001 and n002".to_string(),
        );
        let id2 = queue.enqueue(
            ReviewEventType::BandClippingExceeded,
            ReviewPriority::Normal,
            vec!["n003".to_string()],
            "confidence 0.95 exceeds Asserted band 0.80".to_string(),
        );

        let pending = queue.list_pending();
        assert_eq!(pending.len(), 2);
        // Normal priority: oldest first.
        assert_eq!(pending[0].id, id1);
        assert_eq!(pending[1].id, id2);
    }

    #[test]
    fn test_high_priority_inserted_before_normal() {
        let queue = ReviewQueue::new();
        queue.enqueue(
            ReviewEventType::AmbiguousArbitration,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "normal event".to_string(),
        );
        let high_id = queue.enqueue(
            ReviewEventType::CascadeTruncated,
            ReviewPriority::High,
            vec!["n002".to_string()],
            "cascade truncated at 100 nodes".to_string(),
        );

        let pending = queue.list_pending();
        assert_eq!(pending.len(), 2);
        // High priority should come first.
        assert_eq!(pending[0].id, high_id);
        assert_eq!(pending[0].priority, ReviewPriority::High);
    }

    #[test]
    fn test_approve_event() {
        let queue = ReviewQueue::new();
        let id = queue.enqueue(
            ReviewEventType::AmbiguousArbitration,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "test".to_string(),
        );

        assert_eq!(queue.pending_count(), 1);
        queue.approve(id).unwrap();
        assert_eq!(queue.pending_count(), 0);

        let event = queue.get(id).unwrap();
        assert_eq!(event.status, ReviewStatus::Approved);
    }

    #[test]
    fn test_reject_event() {
        let queue = ReviewQueue::new();
        let id = queue.enqueue(
            ReviewEventType::BandClippingExceeded,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "test".to_string(),
        );

        queue.reject(id).unwrap();
        let event = queue.get(id).unwrap();
        assert_eq!(event.status, ReviewStatus::Rejected);
    }

    #[test]
    fn test_cannot_approve_already_resolved() {
        let queue = ReviewQueue::new();
        let id = queue.enqueue(
            ReviewEventType::AmbiguousArbitration,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "test".to_string(),
        );

        queue.approve(id).unwrap();
        let result = queue.reject(id);
        assert!(result.is_err());
    }

    #[test]
    fn test_auto_resolve() {
        let queue = ReviewQueue::new();
        let id = queue.enqueue(
            ReviewEventType::AmbiguousArbitration,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "test".to_string(),
        );

        queue.auto_resolve(id).unwrap();
        let event = queue.get(id).unwrap();
        assert_eq!(event.status, ReviewStatus::AutoResolved);
        assert!(!event.is_pending());
    }

    #[test]
    fn test_list_all_includes_resolved() {
        let queue = ReviewQueue::new();
        let id1 = queue.enqueue(
            ReviewEventType::AmbiguousArbitration,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "event 1".to_string(),
        );
        let id2 = queue.enqueue(
            ReviewEventType::BandClippingExceeded,
            ReviewPriority::Normal,
            vec!["n002".to_string()],
            "event 2".to_string(),
        );

        queue.approve(id1).unwrap();

        let all = queue.list_all();
        assert_eq!(all.len(), 2);
        // Newest first.
        assert_eq!(all[0].id, id2);
        assert_eq!(all[1].id, id1);
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let queue = ReviewQueue::new();
        assert!(queue.get(999).is_none());
    }

    #[test]
    fn test_pending_count() {
        let queue = ReviewQueue::new();
        assert_eq!(queue.pending_count(), 0);

        let id1 = queue.enqueue(
            ReviewEventType::AmbiguousArbitration,
            ReviewPriority::Normal,
            vec!["n001".to_string()],
            "test 1".to_string(),
        );
        let _id2 = queue.enqueue(
            ReviewEventType::BandClippingExceeded,
            ReviewPriority::Normal,
            vec!["n002".to_string()],
            "test 2".to_string(),
        );

        assert_eq!(queue.pending_count(), 2);
        queue.approve(id1).unwrap();
        assert_eq!(queue.pending_count(), 1);
    }
}
