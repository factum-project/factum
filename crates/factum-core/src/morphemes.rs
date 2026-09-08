//! Morpheme registry — the vocabulary of Factum.
//!
//! Morphemes are the content predicates of Factum, like "shareholder-major",
//! "instance-of", "located-in". Each morpheme has:
//! - A unique ID (u32 index)
//! - A name (e.g., "shareholder-major")
//! - A kind (Entity, Relation, Quantifier, Modal, Temporal)
//! - A type signature
//! - A proposal status (Draft, Review, Adopted)
//!
//! At compile time, seed morphemes are loaded from `morphemes.toml`.
//! At runtime, new morphemes can be registered with ProposalStatus::Draft.

use std::sync::Arc;
use smol_str::SmolStr;
use ahash::AHashMap;
use parking_lot::RwLock;
use crate::types::MorphemeId;

/// Kind of morpheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MorphemeKind {
    /// An entity type (e.g., "Organization", "Person").
    Entity,
    /// A relation between entities (e.g., "shareholder-major").
    Relation,
    /// A quantifier (e.g., "all", "some", "most").
    Quantifier,
    /// A modal operator (e.g., "must", "may", "should").
    Modal,
    /// A temporal operator (e.g., "since", "until", "during").
    Temporal,
}

/// Governance status of a morpheme proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ProposalStatus {
    /// Draft — proposed but not yet reviewed.
    #[default]
    Draft,
    /// Under review.
    Review,
    /// Adopted — stable, can be used in production nodes.
    Adopted,
}

/// Type signature for a morpheme (simplified).
/// Full type system with inference is a future milestone.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct FnType {
    /// Raw signature string like "org:Organization, holder:Person|Org -> Assertion"
    pub raw: SmolStr,
}

impl FnType {
    pub fn new(s: impl Into<SmolStr>) -> Self {
        Self { raw: s.into() }
    }
}

/// Definition of a morpheme.
#[derive(Clone, Debug, PartialEq)]
pub struct MorphemeDef {
    pub id: MorphemeId,
    pub name: SmolStr,
    pub kind: MorphemeKind,
    pub signature: FnType,
    pub doc: SmolStr,
    pub status: ProposalStatus,
}

impl MorphemeDef {
    /// Check if this morpheme is adopted (stable for production use).
    pub fn is_adopted(&self) -> bool {
        self.status == ProposalStatus::Adopted
    }
}

/// Thread-safe morpheme registry.
///
/// Maps name → MorphemeId and MorphemeId → MorphemeDef.
/// Seed morphemes are pre-registered; runtime registration
/// creates Draft-status morphemes.
#[derive(Debug)]
pub struct MorphemeRegistry {
    /// name → MorphemeId
    by_name: RwLock<AHashMap<SmolStr, MorphemeId>>,
    /// MorphemeId → MorphemeDef
    by_id: RwLock<Vec<Arc<MorphemeDef>>>,
}

impl Default for MorphemeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MorphemeRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            by_name: RwLock::new(AHashMap::new()),
            by_id: RwLock::new(Vec::new()),
        }
    }

    /// Create a registry pre-loaded with seed morphemes.
    pub fn with_seeds() -> Self {
        let reg = Self::new();
        for def in seed_morphemes() {
            reg.register(def);
        }
        reg
    }

    /// Register a new morpheme. Returns its assigned ID.
    /// If the name already exists, returns the existing ID.
    pub fn register(&self, def: MorphemeDef) -> MorphemeId {
        // Check if name already exists
        {
            let by_name = self.by_name.read();
            if let Some(&id) = by_name.get(&def.name) {
                return id;
            }
        }

        let mut by_name = self.by_name.write();
        // Double-check after acquiring write lock
        if let Some(&id) = by_name.get(&def.name) {
            return id;
        }

        let mut by_id = self.by_id.write();
        let id = MorphemeId(by_id.len() as u32);
        let mut def = def;
        def.id = id;
        by_id.push(Arc::new(def.clone()));
        by_name.insert(def.name.clone(), id);
        id
    }

    /// Register a morpheme by name (convenience method).
    /// Creates a Draft-status morpheme with empty signature.
    pub fn register_name(&self, name: impl Into<SmolStr>, kind: MorphemeKind) -> MorphemeId {
        self.register(MorphemeDef {
            id: MorphemeId(0), // will be assigned
            name: name.into(),
            kind,
            signature: FnType::default(),
            doc: SmolStr::default(),
            status: ProposalStatus::Draft,
        })
    }

    /// Look up a morpheme by name.
    pub fn lookup(&self, name: &str) -> Option<Arc<MorphemeDef>> {
        let by_name = self.by_name.read();
        let id = *by_name.get(name)?;
        let by_id = self.by_id.read();
        by_id.get(id.0 as usize).cloned()
    }

    /// Look up a morpheme by ID.
    pub fn lookup_id(&self, id: MorphemeId) -> Option<Arc<MorphemeDef>> {
        let by_id = self.by_id.read();
        by_id.get(id.0 as usize).cloned()
    }

    /// Resolve a name to MorphemeId.
    pub fn resolve(&self, name: &str) -> Option<MorphemeId> {
        let by_name = self.by_name.read();
        by_name.get(name).copied()
    }

    /// Get all registered morphemes.
    pub fn all(&self) -> Vec<Arc<MorphemeDef>> {
        self.by_id.read().clone()
    }

    /// Number of registered morphemes.
    pub fn len(&self) -> usize {
        self.by_id.read().len()
    }

    /// Is the registry empty?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Seed morphemes — the initial vocabulary of Factum.
///
/// These cover common entity types, relations, quantifiers,
/// modals, and temporal operators. In production, these would
/// be loaded from morphemes.toml via build.rs.
fn seed_morphemes() -> Vec<MorphemeDef> {
    macro_rules! m {
        ($name:expr, $kind:expr, $sig:expr, $doc:expr) => {
            MorphemeDef {
                id: MorphemeId(0),
                name: SmolStr::new($name),
                kind: $kind,
                signature: FnType::new($sig),
                doc: SmolStr::new($doc),
                status: ProposalStatus::Adopted,
            }
        };
    }

    vec![
        // ── Entity types ──
        m!("organization", MorphemeKind::Entity, "-> EntityType", "An organization entity"),
        m!("person", MorphemeKind::Entity, "-> EntityType", "A person entity"),
        m!("product", MorphemeKind::Entity, "-> EntityType", "A product entity"),
        m!("location", MorphemeKind::Entity, "-> EntityType", "A geographic location"),
        m!("event", MorphemeKind::Entity, "-> EntityType", "An event entity"),

        // ── Relations ──
        m!("instance-of", MorphemeKind::Relation, "entity:Entity, type:EntityType -> Assertion", "Entity is an instance of a type"),
        m!("shareholder-major", MorphemeKind::Relation, "org:Organization, holder:Person|Org, since:Date, stake:Ratio -> Assertion", "Major shareholder relationship"),
        m!("subsidiary-of", MorphemeKind::Relation, "child:Organization, parent:Organization, since:Date -> Assertion", "Subsidiary relationship"),
        m!("located-in", MorphemeKind::Relation, "entity:Entity, location:Location -> Assertion", "Entity is located in a location"),
        m!("founded-on", MorphemeKind::Relation, "org:Organization, date:Date -> Assertion", "Organization founding date"),
        m!("ceo-of", MorphemeKind::Relation, "person:Person, org:Organization, since:Date, until:Date? -> Assertion", "CEO relationship"),
        m!("revenue", MorphemeKind::Relation, "org:Organization, period:DateRange, amount:Dec -> Assertion", "Revenue for a period"),
        m!("employee-count", MorphemeKind::Relation, "org:Organization, date:Date, count:Int -> Assertion", "Employee count"),
        m!("acquired-by", MorphemeKind::Relation, "target:Organization, acquirer:Organization, date:Date, amount:Dec? -> Assertion", "Acquisition relationship"),
        m!("citizen-of", MorphemeKind::Relation, "person:Person, country:Location -> Assertion", "Citizenship"),

        // ── Quantifiers ──
        m!("all", MorphemeKind::Quantifier, "pred:Predicate -> Assertion", "Universal quantifier"),
        m!("some", MorphemeKind::Quantifier, "pred:Predicate -> Assertion", "Existential quantifier"),
        m!("most", MorphemeKind::Quantifier, "pred:Predicate, threshold:Ratio -> Assertion", "Majority quantifier"),

        // ── Modals ──
        m!("must", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Necessity modal"),
        m!("may", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Possibility modal"),
        m!("should", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Recommendation modal"),

        // ── Temporal ──
        m!("since", MorphemeKind::Temporal, "pred:Predicate, from:Date -> Assertion", "Valid since a date"),
        m!("until", MorphemeKind::Temporal, "pred:Predicate, until:Date -> Assertion", "Valid until a date"),
        m!("during", MorphemeKind::Temporal, "pred:Predicate, interval:DateRange -> Assertion", "Valid during an interval"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_basic() {
        let reg = MorphemeRegistry::new();
        let id = reg.register_name("test-morpheme", MorphemeKind::Relation);
        assert_eq!(id, MorphemeId(0));

        let def = reg.lookup("test-morpheme").unwrap();
        assert_eq!(def.name, "test-morpheme");
        assert_eq!(def.kind, MorphemeKind::Relation);
        assert_eq!(def.status, ProposalStatus::Draft);
    }

    #[test]
    fn test_registry_dedup() {
        let reg = MorphemeRegistry::new();
        let id1 = reg.register_name("foo", MorphemeKind::Entity);
        let id2 = reg.register_name("foo", MorphemeKind::Entity);
        assert_eq!(id1, id2);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn test_seed_morphemes() {
        let reg = MorphemeRegistry::with_seeds();
        assert!(reg.len() >= 20);

        let def = reg.lookup("shareholder-major").unwrap();
        assert_eq!(def.kind, MorphemeKind::Relation);
        assert!(def.is_adopted());

        let id = reg.resolve("instance-of").unwrap();
        assert_eq!(reg.lookup_id(id).unwrap().name, "instance-of");
    }
}
