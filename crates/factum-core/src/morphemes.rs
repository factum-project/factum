//! Morpheme registry — the vocabulary of Factum.
//!
//! Morphemes are the content predicates of Factum, like "shareholder-major",
//! "instance-of", "located-in". Each morpheme has:
//! - A unique ID (u32 index)
//! - A name (e.g., "shareholder-major")
//! - A kind (Entity, Relation, Quantifier, Modal, Temporal, Status, Action, Attribute, Classification)
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
    /// A status or lifecycle state (e.g., "active", "deprecated", "draft").
    Status,
    /// An action or operation (e.g., "published", "reviewed", "deployed").
    Action,
    /// An attribute or property of an entity (e.g., "name", "version", "url").
    Attribute,
    /// A classification or category tag (e.g., "bug", "feature", "security").
    Classification,
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
/// These cover common entity types, relations, quantifiers, modals,
/// temporal operators, status states, actions, attributes, and classifications.
/// In production, these would be loaded from morphemes.toml via build.rs.
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
        // ══ Entity Types (30) ══
        m!("organization", MorphemeKind::Entity, "-> EntityType", "An organization entity"),
        m!("person", MorphemeKind::Entity, "-> EntityType", "A person entity"),
        m!("product", MorphemeKind::Entity, "-> EntityType", "A product entity"),
        m!("location", MorphemeKind::Entity, "-> EntityType", "A geographic location"),
        m!("event", MorphemeKind::Entity, "-> EntityType", "An event entity"),
        m!("document", MorphemeKind::Entity, "-> EntityType", "A document or file"),
        m!("project", MorphemeKind::Entity, "-> EntityType", "A software project"),
        m!("repository", MorphemeKind::Entity, "-> EntityType", "A code repository"),
        m!("commit", MorphemeKind::Entity, "-> EntityType", "A version control commit"),
        m!("branch", MorphemeKind::Entity, "-> EntityType", "A version control branch"),
        m!("release", MorphemeKind::Entity, "-> EntityType", "A software release version"),
        m!("module", MorphemeKind::Entity, "-> EntityType", "A software module or crate"),
        m!("function", MorphemeKind::Entity, "-> EntityType", "A function or method"),
        m!("file", MorphemeKind::Entity, "-> EntityType", "A file in a repository"),
        m!("issue", MorphemeKind::Entity, "-> EntityType", "A bug report or feature request"),
        m!("pr", MorphemeKind::Entity, "-> EntityType", "A pull request"),
        m!("milestone", MorphemeKind::Entity, "-> EntityType", "A project milestone"),
        m!("team", MorphemeKind::Entity, "-> EntityType", "A team or working group"),
        m!("role", MorphemeKind::Entity, "-> EntityType", "A user role for permissions"),
        m!("tool", MorphemeKind::Entity, "-> EntityType", "A software tool or CLI"),
        m!("service", MorphemeKind::Entity, "-> EntityType", "A running service or process"),
        m!("database", MorphemeKind::Entity, "-> EntityType", "A database instance"),
        m!("config", MorphemeKind::Entity, "-> EntityType", "A configuration setting"),
        m!("license", MorphemeKind::Entity, "-> EntityType", "A software license"),
        m!("standard", MorphemeKind::Entity, "-> EntityType", "A technical standard or spec"),
        m!("protocol", MorphemeKind::Entity, "-> EntityType", "A communication protocol"),
        m!("api", MorphemeKind::Entity, "-> EntityType", "An API endpoint or interface"),
        m!("dataset", MorphemeKind::Entity, "-> EntityType", "A collection of data"),
        m!("model-ent", MorphemeKind::Entity, "-> EntityType", "An AI model"),
        m!("agent", MorphemeKind::Entity, "-> EntityType", "An AI agent instance"),

        // ══ Relations — Organizational (15) ══
        m!("instance-of", MorphemeKind::Relation, "entity:Entity, type:EntityType -> Assertion", "Entity is an instance of a type"),
        m!("subsidiary-of", MorphemeKind::Relation, "child:Organization, parent:Organization, since:Date -> Assertion", "Subsidiary relationship"),
        m!("shareholder-major", MorphemeKind::Relation, "org:Organization, holder:Person|Org, since:Date, stake:Ratio -> Assertion", "Major shareholder relationship"),
        m!("acquired-by", MorphemeKind::Relation, "target:Organization, acquirer:Organization, date:Date, amount:Dec? -> Assertion", "Acquisition relationship"),
        m!("merged-into", MorphemeKind::Relation, "source:Organization, target:Organization, date:Date -> Assertion", "Merger relationship"),
        m!("partner-of", MorphemeKind::Relation, "orgA:Organization, orgB:Organization, since:Date -> Assertion", "Partnership relationship"),
        m!("competitor-of", MorphemeKind::Relation, "orgA:Organization, orgB:Organization -> Assertion", "Competition relationship"),
        m!("division-of", MorphemeKind::Relation, "unit:Organization, parent:Organization -> Assertion", "Division relationship"),
        m!("member-of", MorphemeKind::Relation, "person:Person, org:Organization, since:Date -> Assertion", "Membership relationship"),
        m!("employee-of", MorphemeKind::Relation, "person:Person, org:Organization, since:Date, until:Date? -> Assertion", "Employment relationship"),
        m!("reports-to", MorphemeKind::Relation, "person:Person, manager:Person, since:Date -> Assertion", "Reporting hierarchy"),
        m!("team-member-of", MorphemeKind::Relation, "person:Person, team:Team, since:Date -> Assertion", "Team membership"),
        m!("founded-by", MorphemeKind::Relation, "org:Organization, person:Person, date:Date -> Assertion", "Founder relationship"),
        m!("board-member-of", MorphemeKind::Relation, "person:Person, org:Organization, since:Date -> Assertion", "Board membership"),
        m!("investor-in", MorphemeKind::Relation, "investor:Entity, target:Organization, amount:Dec, date:Date -> Assertion", "Investment relationship"),

        // ══ Relations — People (10) ══
        m!("ceo-of", MorphemeKind::Relation, "person:Person, org:Organization, since:Date, until:Date? -> Assertion", "CEO relationship"),
        m!("cto-of", MorphemeKind::Relation, "person:Person, org:Organization, since:Date, until:Date? -> Assertion", "CTO relationship"),
        m!("cfo-of", MorphemeKind::Relation, "person:Person, org:Organization, since:Date, until:Date? -> Assertion", "CFO relationship"),
        m!("author-of", MorphemeKind::Relation, "person:Person, work:Document, date:Date -> Assertion", "Authorship relationship"),
        m!("reviewer-of", MorphemeKind::Relation, "person:Person, work:Entity, date:Date -> Assertion", "Review relationship"),
        m!("contributor-to", MorphemeKind::Relation, "person:Person, project:Project, since:Date -> Assertion", "Contributor relationship"),
        m!("maintainer-of", MorphemeKind::Relation, "person:Person, project:Project, since:Date -> Assertion", "Maintainer relationship"),
        m!("citizen-of", MorphemeKind::Relation, "person:Person, country:Location -> Assertion", "Citizenship"),
        m!("resides-in", MorphemeKind::Relation, "person:Person, location:Location, since:Date -> Assertion", "Residence"),
        m!("educated-at", MorphemeKind::Relation, "person:Person, org:Organization, degree:String? -> Assertion", "Education"),

        // ══ Relations — Spatial/Location (8) ══
        m!("located-in", MorphemeKind::Relation, "entity:Entity, location:Location -> Assertion", "Entity is located in a location"),
        m!("headquartered-in", MorphemeKind::Relation, "org:Organization, location:Location, since:Date -> Assertion", "Headquarters location"),
        m!("operates-in", MorphemeKind::Relation, "org:Organization, location:Location, since:Date -> Assertion", "Operational presence"),
        m!("country-of", MorphemeKind::Relation, "location:Location, country:Location -> Assertion", "Country containing a location"),
        m!("region-of", MorphemeKind::Relation, "location:Location, region:Location -> Assertion", "Region containing a location"),
        m!("bordered-by", MorphemeKind::Relation, "location:Location, neighbor:Location -> Assertion", "Bordering locations"),
        m!("capital-of", MorphemeKind::Relation, "city:Location, country:Location -> Assertion", "Capital city"),
        m!("address-at", MorphemeKind::Relation, "entity:Entity, street:String, city:Location -> Assertion", "Street address"),

        // ══ Relations — Financial (12) ══
        m!("revenue", MorphemeKind::Relation, "org:Organization, period:Date, amount:Dec -> Assertion", "Revenue for a period"),
        m!("profit", MorphemeKind::Relation, "org:Organization, period:Date, amount:Dec -> Assertion", "Profit for a period"),
        m!("loss", MorphemeKind::Relation, "org:Organization, period:Date, amount:Dec -> Assertion", "Loss for a period"),
        m!("asset-value", MorphemeKind::Relation, "org:Organization, date:Date, amount:Dec -> Assertion", "Total asset value"),
        m!("liability", MorphemeKind::Relation, "org:Organization, date:Date, amount:Dec -> Assertion", "Liability amount"),
        m!("market-cap", MorphemeKind::Relation, "org:Organization, date:Date, amount:Dec -> Assertion", "Market capitalization"),
        m!("valuation", MorphemeKind::Relation, "org:Organization, date:Date, amount:Dec, stage:String -> Assertion", "Valuation at funding stage"),
        m!("funding-round", MorphemeKind::Relation, "org:Organization, date:Date, amount:Dec, stage:String -> Assertion", "Funding round"),
        m!("debt", MorphemeKind::Relation, "org:Organization, date:Date, amount:Dec -> Assertion", "Debt amount"),
        m!("employee-count", MorphemeKind::Relation, "org:Organization, date:Date, count:Int -> Assertion", "Employee count"),
        m!("budget", MorphemeKind::Relation, "org:Organization, period:Date, amount:Dec -> Assertion", "Budget for a period"),
        m!("cost", MorphemeKind::Relation, "project:Project, date:Date, amount:Dec -> Assertion", "Project cost"),

        // ══ Relations — Product/Project (12) ══
        m!("developed-by", MorphemeKind::Relation, "product:Product, org:Organization -> Assertion", "Product developer"),
        m!("owned-by", MorphemeKind::Relation, "entity:Entity, owner:Entity -> Assertion", "Ownership"),
        m!("uses-tool", MorphemeKind::Relation, "project:Project, tool:Tool, since:Date -> Assertion", "Tool dependency"),
        m!("depends-on", MorphemeKind::Relation, "module:Module, dep:Module, version:String -> Assertion", "Module dependency"),
        m!("built-with", MorphemeKind::Relation, "project:Project, tool:Tool -> Assertion", "Build tool"),
        m!("deployed-on", MorphemeKind::Relation, "service:Service, platform:String, date:Date -> Assertion", "Deployment platform"),
        m!("hosts", MorphemeKind::Relation, "service:Service, module:Module -> Assertion", "Hosting relationship"),
        m!("imports", MorphemeKind::Relation, "module:Module, dep:Module -> Assertion", "Import dependency"),
        m!("extends", MorphemeKind::Relation, "module:Module, base:Module -> Assertion", "Extension relationship"),
        m!("replaces", MorphemeKind::Relation, "newItem:Entity, oldItem:Entity, date:Date -> Assertion", "Replacement relationship"),
        m!("compatible-with", MorphemeKind::Relation, "module:Module, other:Module, version:String -> Assertion", "Compatibility"),
        m!("deprecated-by", MorphemeKind::Relation, "oldItem:Entity, newItem:Entity, date:Date -> Assertion", "Deprecation replacement"),

        // ══ Relations — Version Control (10) ══
        m!("committed-to", MorphemeKind::Relation, "commit:Commit, branch:Branch, date:Date -> Assertion", "Commit target branch"),
        m!("authored-by", MorphemeKind::Relation, "commit:Commit, person:Person, date:Date -> Assertion", "Commit author"),
        m!("merged-into-branch", MorphemeKind::Relation, "pr:PR, branch:Branch, date:Date -> Assertion", "PR merge target"),
        m!("released-in", MorphemeKind::Relation, "commit:Commit, release:Release -> Assertion", "Commit included in release"),
        m!("tagged-as", MorphemeKind::Relation, "commit:Commit, tag:String, date:Date -> Assertion", "Commit tag"),
        m!("fixes-issue", MorphemeKind::Relation, "pr:PR, issue:Issue -> Assertion", "PR fixes issue"),
        m!("references-issue", MorphemeKind::Relation, "commit:Commit, issue:Issue -> Assertion", "Commit references issue"),
        m!("blocks-issue", MorphemeKind::Relation, "issue:Issue, blocked:Issue -> Assertion", "Issue blocking"),
        m!("duplicates-issue", MorphemeKind::Relation, "issue:Issue, original:Issue -> Assertion", "Issue duplication"),
        m!("assigned-to", MorphemeKind::Relation, "issue:Issue, person:Person, date:Date -> Assertion", "Issue assignment"),

        // ══ Relations — Document/Knowledge (10) ══
        m!("documented-in", MorphemeKind::Relation, "entity:Entity, doc:Document -> Assertion", "Documentation location"),
        m!("described-by", MorphemeKind::Relation, "entity:Entity, doc:Document -> Assertion", "Description source"),
        m!("specified-by", MorphemeKind::Relation, "entity:Entity, standard:Standard -> Assertion", "Specification"),
        m!("cites", MorphemeKind::Relation, "doc:Document, ref:Document -> Assertion", "Citation"),
        m!("supersedes", MorphemeKind::Relation, "newDoc:Document, oldDoc:Document, date:Date -> Assertion", "Document supersession"),
        m!("translates", MorphemeKind::Relation, "doc:Document, source:Document, lang:String -> Assertion", "Translation"),
        m!("extracted-from", MorphemeKind::Relation, "fact:Entity, source:Document, span:String -> Assertion", "Knowledge extraction source"),
        m!("summarizes", MorphemeKind::Relation, "doc:Document, source:Document -> Assertion", "Summary relationship"),
        m!("published-in", MorphemeKind::Relation, "doc:Document, venue:String, date:Date -> Assertion", "Publication venue"),
        m!("licensed-under", MorphemeKind::Relation, "project:Project, license:License -> Assertion", "License"),

        // ══ Relations — Temporal/Founding (5) ══
        m!("founded-on", MorphemeKind::Relation, "org:Organization, date:Date -> Assertion", "Organization founding date"),
        m!("dissolved-on", MorphemeKind::Relation, "org:Organization, date:Date -> Assertion", "Organization dissolution date"),
        m!("started-on", MorphemeKind::Relation, "project:Project, date:Date -> Assertion", "Project start date"),
        m!("completed-on", MorphemeKind::Relation, "project:Project, date:Date -> Assertion", "Project completion date"),
        m!("released-on", MorphemeKind::Relation, "product:Product, version:String, date:Date -> Assertion", "Release date"),

        // ══ Relations — Agent Memory (8) ══
        m!("learned-from", MorphemeKind::Relation, "fact:Entity, source:Entity, date:Date -> Assertion", "Knowledge acquisition source"),
        m!("believes", MorphemeKind::Relation, "agent:Agent, fact:Entity, since:Date -> Assertion", "Agent belief"),
        m!("knows-about", MorphemeKind::Relation, "agent:Agent, entity:Entity, since:Date -> Assertion", "Agent knowledge"),
        m!("prefers", MorphemeKind::Relation, "agent:Agent, entity:Entity, since:Date -> Assertion", "Agent preference"),
        m!("dislikes", MorphemeKind::Relation, "agent:Agent, entity:Entity, since:Date -> Assertion", "Agent dislike"),
        m!("recalls", MorphemeKind::Relation, "agent:Agent, event:Event, date:Date -> Assertion", "Agent memory recall"),
        m!("forgot", MorphemeKind::Relation, "agent:Agent, fact:Entity, date:Date -> Assertion", "Agent memory loss"),
        m!("uncertain-about", MorphemeKind::Relation, "agent:Agent, fact:Entity, since:Date -> Assertion", "Agent uncertainty"),

        // ══ Relations — Permission/Access (5) ══
        m!("has-role", MorphemeKind::Relation, "person:Person, role:Role, since:Date -> Assertion", "Role assignment"),
        m!("has-permission", MorphemeKind::Relation, "person:Person, action:String, resource:Entity -> Assertion", "Permission grant"),
        m!("denied-permission", MorphemeKind::Relation, "person:Person, action:String, resource:Entity -> Assertion", "Permission denial"),
        m!("owner-of", MorphemeKind::Relation, "person:Person, entity:Entity, since:Date -> Assertion", "Entity ownership"),
        m!("shared-with", MorphemeKind::Relation, "entity:Entity, person:Person, since:Date -> Assertion", "Shared access"),

        // ══ Relations — Cause/Effect (5) ══
        m!("caused-by", MorphemeKind::Relation, "event:Event, cause:Entity -> Assertion", "Causal relationship"),
        m!("affects", MorphemeKind::Relation, "event:Event, entity:Entity -> Assertion", "Impact relationship"),
        m!("fixes", MorphemeKind::Relation, "pr:PR, issue:Issue -> Assertion", "Fix relationship"),
        m!("triggers", MorphemeKind::Relation, "event:Event, consequence:Event -> Assertion", "Trigger relationship"),
        m!("prevents", MorphemeKind::Relation, "action:String, event:Event -> Assertion", "Prevention relationship"),

        // ══ Quantifiers (6) ══
        m!("all", MorphemeKind::Quantifier, "pred:Predicate -> Assertion", "Universal quantifier"),
        m!("some", MorphemeKind::Quantifier, "pred:Predicate -> Assertion", "Existential quantifier"),
        m!("most", MorphemeKind::Quantifier, "pred:Predicate, threshold:Ratio -> Assertion", "Majority quantifier"),
        m!("none", MorphemeKind::Quantifier, "pred:Predicate -> Assertion", "Negative universal"),
        m!("at-least", MorphemeKind::Quantifier, "pred:Predicate, count:Int -> Assertion", "Minimum count quantifier"),
        m!("at-most", MorphemeKind::Quantifier, "pred:Predicate, count:Int -> Assertion", "Maximum count quantifier"),

        // ══ Modals (6) ══
        m!("must", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Necessity modal"),
        m!("may", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Possibility modal"),
        m!("should", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Recommendation modal"),
        m!("must-not", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Prohibition"),
        m!("might", MorphemeKind::Modal, "pred:Predicate -> Assertion", "Weak possibility"),
        m!("intends-to", MorphemeKind::Modal, "agent:Agent, action:String -> Assertion", "Intention"),

        // ══ Temporal (8) ══
        m!("since", MorphemeKind::Temporal, "pred:Predicate, from:Date -> Assertion", "Valid since a date"),
        m!("until", MorphemeKind::Temporal, "pred:Predicate, until:Date -> Assertion", "Valid until a date"),
        m!("during", MorphemeKind::Temporal, "pred:Predicate, interval:Date, end:Date -> Assertion", "Valid during an interval"),
        m!("before", MorphemeKind::Temporal, "pred:Predicate, until:Date -> Assertion", "Valid before a date"),
        m!("after", MorphemeKind::Temporal, "pred:Predicate, from:Date -> Assertion", "Valid after a date"),
        m!("occurred-on", MorphemeKind::Temporal, "event:Event, date:Date -> Assertion", "Event occurrence date"),
        m!("scheduled-for", MorphemeKind::Temporal, "event:Event, date:Date -> Assertion", "Scheduled date"),
        m!("last-seen", MorphemeKind::Temporal, "entity:Entity, date:Date -> Assertion", "Last observation date"),

        // ══ Status (20) ══
        m!("active", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is active"),
        m!("inactive", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is inactive"),
        m!("deprecated", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is deprecated"),
        m!("draft", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is in draft state"),
        m!("review", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is under review"),
        m!("adopted", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is adopted/stable"),
        m!("experimental", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is experimental"),
        m!("stable", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is stable"),
        m!("beta", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is in beta"),
        m!("alpha", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is in alpha"),
        m!("released-status", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is released"),
        m!("unreleased", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is not yet released"),
        m!("archived", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is archived"),
        m!("deleted-status", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is deleted"),
        m!("blocked", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is blocked"),
        m!("approved", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is approved"),
        m!("rejected", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is rejected"),
        m!("pending", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is pending"),
        m!("resolved", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Entity is resolved"),
        m!("wip", MorphemeKind::Status, "entity:Entity, since:Date -> Assertion", "Work in progress"),

        // ══ Actions (20) ══
        m!("created", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was created"),
        m!("modified", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was modified"),
        m!("deleted-action", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was deleted"),
        m!("published", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was published"),
        m!("deployed", MorphemeKind::Action, "service:Service, by:Person, date:Date, env:String -> Assertion", "Service was deployed"),
        m!("reviewed", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was reviewed"),
        m!("approved-action", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was approved"),
        m!("rejected-action", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was rejected"),
        m!("merged", MorphemeKind::Action, "pr:PR, by:Person, date:Date -> Assertion", "PR was merged"),
        m!("closed", MorphemeKind::Action, "issue:Issue, by:Person, date:Date -> Assertion", "Issue was closed"),
        m!("opened", MorphemeKind::Action, "issue:Issue, by:Person, date:Date -> Assertion", "Issue was opened"),
        m!("assigned", MorphemeKind::Action, "issue:Issue, to:Person, by:Person, date:Date -> Assertion", "Issue was assigned"),
        m!("labeled", MorphemeKind::Action, "issue:Issue, label:String, by:Person, date:Date -> Assertion", "Issue was labeled"),
        m!("released-action", MorphemeKind::Action, "product:Product, version:String, by:Person, date:Date -> Assertion", "Release was published"),
        m!("registered", MorphemeKind::Action, "entity:Entity, by:Person, date:Date, registry:String -> Assertion", "Entity was registered"),
        m!("installed", MorphemeKind::Action, "tool:Tool, by:Person, date:Date -> Assertion", "Tool was installed"),
        m!("executed", MorphemeKind::Action, "tool:Tool, by:Person, date:Date, result:String -> Assertion", "Tool was executed"),
        m!("tested", MorphemeKind::Action, "module:Module, by:Person, date:Date, result:String -> Assertion", "Module was tested"),
        m!("documented-action", MorphemeKind::Action, "entity:Entity, by:Person, date:Date -> Assertion", "Entity was documented"),
        m!("announced", MorphemeKind::Action, "entity:Entity, by:Person, date:Date, channel:String -> Assertion", "Entity was announced"),

        // ══ Attributes (20) ══
        m!("name", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Entity name"),
        m!("version", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Entity version"),
        m!("url", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Entity URL"),
        m!("description", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Entity description"),
        m!("language", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Programming language"),
        m!("license-attr", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "License type"),
        m!("platform", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Target platform"),
        m!("category", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Category tag"),
        m!("priority", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Priority level"),
        m!("severity", MorphemeKind::Attribute, "issue:Issue, value:String -> Assertion", "Issue severity"),
        m!("size", MorphemeKind::Attribute, "entity:Entity, value:Dec, unit:String -> Assertion", "Entity size"),
        m!("count-attr", MorphemeKind::Attribute, "entity:Entity, value:Int -> Assertion", "Numeric count"),
        m!("color", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Color value"),
        m!("format", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Data format"),
        m!("encoding", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Text encoding"),
        m!("checksum", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Checksum hash"),
        m!("path", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "File path"),
        m!("port", MorphemeKind::Attribute, "service:Service, value:Int -> Assertion", "Network port"),
        m!("env-var", MorphemeKind::Attribute, "config:Config, key:String, value:String -> Assertion", "Environment variable"),
        m!("tag", MorphemeKind::Attribute, "entity:Entity, value:String -> Assertion", "Generic tag"),

        // ══ Classification (20) ══
        m!("bug", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Issue is a bug"),
        m!("feature", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Issue is a feature request"),
        m!("enhancement", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Issue is an enhancement"),
        m!("security", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Security issue"),
        m!("performance", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Performance issue"),
        m!("docs-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Documentation issue"),
        m!("refactor", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Refactoring task"),
        m!("test-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Testing task"),
        m!("ci-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "CI/CD issue"),
        m!("build-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Build system issue"),
        m!("dependency-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Dependency issue"),
        m!("breaking-change", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Breaking change"),
        m!("good-first-issue", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Good first issue"),
        m!("help-wanted", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Help wanted"),
        m!("wontfix", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Won't fix"),
        m!("duplicate-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Duplicate issue"),
        m!("invalid-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Invalid issue"),
        m!("question-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Question issue"),
        m!("discussion-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Discussion issue"),
        m!("design-cls", MorphemeKind::Classification, "issue:Issue, since:Date -> Assertion", "Design issue"),
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
        assert!(reg.len() >= 200);

        let def = reg.lookup("shareholder-major").unwrap();
        assert_eq!(def.kind, MorphemeKind::Relation);
        assert!(def.is_adopted());

        let id = reg.resolve("instance-of").unwrap();
        assert_eq!(reg.lookup_id(id).unwrap().name, "instance-of");
    }
}
