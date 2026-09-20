//! Core Factum data types.
//!
//! ## Node 七元组
//! Every piece of knowledge in Factum is a `Node` containing seven fields:
//! 1. `id` — unique identifier
//! 2. `morphemes` / `predicate` — content (what is being asserted)
//! 3. `validity` — temporal validity window
//! 4. `provenance` — where this knowledge came from (audit chain)
//! 5. `confidence` — numeric confidence [0,1]
//! 6. `authority` — source authority weight [0,1]
//! 7. `permissions` — access control tag
//!
//! Plus `deps` for derived-node invalidation propagation.

use std::sync::Arc;
use chrono::{DateTime, Utc, NaiveDate};
use smol_str::SmolStr;

// ─── Identifiers ────────────────────────────────────────────

/// Unique node identifier. Either a human-readable string ("c8842") or ULID.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub Arc<str>);

impl NodeId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(Arc::from(s.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for NodeId {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

impl serde::Serialize for NodeId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for NodeId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::new(s))
    }
}

/// Typed entity identifier (e.g., "ACME-CORP", "ORG:000123").
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(pub Arc<str>);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(Arc::from(s.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl serde::Serialize for EntityId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for EntityId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::new(s))
    }
}

/// Document identifier for provenance tracking.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DocId(pub Arc<str>);

impl DocId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(Arc::from(s.into()))
    }
}

impl serde::Serialize for DocId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for DocId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::new(s))
    }
}

/// Byte span within a document [start, end).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// Model reference for Extracted provenance.
/// Extracted nodes MUST carry model + version — this is non-optional.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ModelRef {
    pub name: SmolStr,
    pub version: SmolStr,
}

/// Principal (who asserted or extracted this knowledge).
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Principal(pub SmolStr);

/// Rule identifier for Derived provenance.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RuleId(pub SmolStr);

// ─── Validity ───────────────────────────────────────────────

/// Temporal validity of a node.
#[derive(Clone, Copy, Debug, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum Validity {
    /// Always valid.
    #[default]
    Forever,
    /// Valid within a time window.
    /// `until = None` means "valid from `from` onwards (open-ended)".
    Window {
        from: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    },
}

impl Validity {
    /// Check if this node is valid at the given time.
    pub fn is_valid_at(&self, t: DateTime<Utc>) -> bool {
        match self {
            Validity::Forever => true,
            Validity::Window { from, until } => {
                *from <= t && until.is_none_or(|u| t < u)
            }
        }
    }

    /// Check if this node is valid right now.
    pub fn is_now(&self) -> bool {
        self.is_valid_at(Utc::now())
    }
}

// ─── Provenance ─────────────────────────────────────────────

/// Provenance — the audit trail for every node.
///
/// This is the foundation of Factum's trust model. Every node must declare
/// where its knowledge came from. `Extracted` nodes MUST carry a model
/// reference — serialization is illegal without it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Provenance {
    /// Verbatim quote from a document. The original text can be precisely
    /// reconstructed using (doc, span).
    Verbatim {
        doc: DocId,
        span: Span,
    },
    /// Human-written summary. `span` points to the summarized region.
    Summary {
        doc: DocId,
        span: Span,
    },
    /// LLM-extracted knowledge. MUST carry extraction model + version.
    /// This is non-optional — it's the basis of the audit chain.
    Extracted {
        doc: DocId,
        span: Span,
        model: ModelRef,
    },
    /// Formal derivation (Lean/arithmetic/solver proof).
    /// `from` is the source node, `rule` is the derivation rule used.
    Derived {
        from: NodeId,
        rule: RuleId,
    },
    /// Directly asserted by a human or external system.
    Asserted {
        by: Principal,
    },
}

impl Default for Provenance {
    fn default() -> Self {
        Provenance::Asserted {
            by: Principal(SmolStr::new("system")),
        }
    }
}

// ─── Confidence & Authority ─────────────────────────────────

/// Confidence score in [0, 1]. Stored as f32 but validated on construction.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct Confidence(pub f32);

impl Confidence {
    pub fn new(v: f32) -> Result<Self, CoreError> {
        if (0.0..=1.0).contains(&v) {
            Ok(Self(v))
        } else {
            Err(CoreError::ConfidenceOutOfRange(v))
        }
    }

    pub fn certain() -> Self {
        Self(1.0)
    }
}

impl Default for Confidence {
    fn default() -> Self {
        // Conservative default (matching Asserted provenance).
        // Callers should use calibration::default_confidence_for_provenance()
        // for provenance-specific defaults. This fallback ensures that even
        // if a code path bypasses that function, the result is conservative
        // rather than falsely certain (previous default was 1.0).
        Self(0.60)
    }
}

/// Authority weight in [0, 1]. Reflects the trustworthiness of the source.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct Authority(pub f32);

impl Authority {
    pub fn new(v: f32) -> Result<Self, CoreError> {
        if (0.0..=1.0).contains(&v) {
            Ok(Self(v))
        } else {
            Err(CoreError::AuthorityOutOfRange(v))
        }
    }

    pub fn max() -> Self {
        Self(1.0)
    }
}

impl Default for Authority {
    fn default() -> Self {
        Self(0.5)
    }
}

// ─── Literal ────────────────────────────────────────────────

/// Literal values with lossless numeric precision.
///
/// # Critical Design Decision
/// All numeric values use `Dec(i128, u8)` — mantissa × 10^(-scale).
/// This completely avoids floating-point errors. Using f64 for
/// money/ratios would destroy the determinism required by verification
/// hooks (see §3.4 of the spec). This is non-negotiable.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Literal {
    /// Decimal number: mantissa × 10^(-scale).
    /// Max precision: 38 digits (i128 can hold ~38 decimal digits).
    /// Example: 230.50 → Dec(23050, 2)
    Dec(i128, u8),
    /// UTF-8 string.
    Str(SmolStr),
    /// Calendar date (no timezone).
    Date(NaiveDate),
    /// Duration in nanoseconds.
    Dur(std::time::Duration),
    /// Boolean.
    Bool(bool),
    /// URI reference.
    Uri(SmolStr),
}

impl Literal {
    /// Create a decimal literal from a string like "230.50".
    pub fn dec_from_str(s: &str) -> Result<Self, CoreError> {
        let s = s.trim();
        let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
            (-1i128, r)
        } else if let Some(r) = s.strip_prefix('+') {
            (1i128, r)
        } else {
            (1i128, s)
        };

        let (int_part, frac_part) = match rest.split_once('.') {
            Some((i, f)) => (i, f),
            None => (rest, ""),
        };

        if int_part.is_empty() && frac_part.is_empty() {
            return Err(CoreError::InvalidDecimal(s.to_string()));
        }

        // Validate digits
        if !int_part.chars().all(|c| c.is_ascii_digit()) ||
           !frac_part.chars().all(|c| c.is_ascii_digit())
        {
            return Err(CoreError::InvalidDecimal(s.to_string()));
        }

        let scale = frac_part.len() as u8;
        let combined = format!("{}{}", int_part, frac_part);
        let mantissa: i128 = combined.parse().map_err(|_| CoreError::InvalidDecimal(s.to_string()))?;
        let mantissa = mantissa.checked_mul(sign).ok_or(CoreError::DecimalOverflow)?;

        Ok(Self::Dec(mantissa, scale))
    }

    /// Convert to string representation (canonical form).
    pub fn to_canonical_string(&self) -> String {
        match self {
            Literal::Dec(mantissa, scale) => {
                let neg = *mantissa < 0;
                let abs = mantissa.unsigned_abs();
                let s = abs.to_string();
                let scale = *scale as usize;
                let result = if scale == 0 {
                    s
                } else if s.len() <= scale {
                    // Need leading zeros: 0.00XXX
                    let zeros = scale - s.len();
                    format!("0.{}{}", "0".repeat(zeros), s)
                } else {
                    let (int_part, frac_part) = s.split_at(s.len() - scale);
                    format!("{}.{}", int_part, frac_part)
                };
                if neg { format!("-{}", result) } else { result }
            }
            Literal::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            Literal::Date(d) => d.format("%Y-%m-%d").to_string(),
            Literal::Dur(d) => format!("{}ns", d.as_nanos()),
            Literal::Bool(b) => b.to_string(),
            Literal::Uri(u) => format!("<{}>", u),
        }
    }

    /// Get the decimal value as an f64 (for comparison only, not for storage).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Literal::Dec(m, s) => Some(*m as f64 / 10f64.powi(*s as i32)),
            Literal::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
}

// ─── Morpheme Reference ─────────────────────────────────────

/// Global morpheme registry index.
/// Morphemes are content predicates like "shareholder-major".
/// The registry maps MorphemeId → MorphemeDef (name, kind, signature).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MorphemeId(pub u32);

// ─── Terms ──────────────────────────────────────────────────

/// A term in a predicate. Can be a variable, entity, literal,
/// nested predicate, or list.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Term {
    /// Variable: ?p, ?x
    Var(SmolStr),
    /// Typed entity reference
    Ent(EntityId),
    /// Literal value (Dec, Str, Date, etc.)
    Lit(Literal),
    /// Nested predicate (compound term)
    Compound(Box<Predicate>),
    /// List of terms
    List(Vec<Term>),
}

impl Term {
    pub fn var(s: impl Into<String>) -> Self {
        Term::Var(SmolStr::from(s.into()))
    }
    pub fn ent(s: impl Into<String>) -> Self {
        Term::Ent(EntityId::new(s.into()))
    }
    pub fn lit(l: Literal) -> Self {
        Term::Lit(l)
    }
}

// ─── Predicate ──────────────────────────────────────────────

/// Content predicate: the actual assertion of a node.
///
/// Example: (shareholder-major ACME-CORP ?p :valid [now])
/// - head: MorphemeId for "shareholder-major"
/// - args: [Ent("ACME-CORP"), Var("?p")]
/// - named: [("valid", List([Var("now")]))]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Predicate {
    /// The morpheme (predicate head). Can be either an ID (resolved)
    /// or a name (unresolved, resolved during parsing).
    pub head: PredicateHead,
    /// Positional arguments.
    pub args: Vec<Term>,
    /// Named arguments (keyword args). Must appear after positional args.
    pub named: Vec<(SmolStr, Term)>,
}

/// A predicate head can be either a resolved MorphemeId or an
/// unresolved name string (before registry lookup).
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PredicateHead {
    /// Resolved to a morpheme ID from the registry.
    Id(MorphemeId),
    /// Unresolved name (e.g., "shareholder-major").
    /// Resolved to Id during or after parsing.
    Name(SmolStr),
}

impl Predicate {
    pub fn new(head: impl Into<PredicateHead>) -> Self {
        Self {
            head: head.into(),
            args: Vec::new(),
            named: Vec::new(),
        }
    }

    pub fn with_args(mut self, args: Vec<Term>) -> Self {
        self.args = args;
        self
    }

    pub fn with_named(mut self, key: impl Into<SmolStr>, val: Term) -> Self {
        self.named.push((key.into(), val));
        self
    }
}

impl From<MorphemeId> for PredicateHead {
    fn from(id: MorphemeId) -> Self {
        PredicateHead::Id(id)
    }
}

impl From<SmolStr> for PredicateHead {
    fn from(s: SmolStr) -> Self {
        PredicateHead::Name(s)
    }
}

impl From<&str> for PredicateHead {
    fn from(s: &str) -> Self {
        PredicateHead::Name(SmolStr::new(s))
    }
}

// ─── Permission Tag ─────────────────────────────────────────

/// Permission tag for access control.
/// Uses a bitmask for efficient intersection at the index layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct PermissionTag(pub u32);

impl PermissionTag {
    pub const PUBLIC: Self = Self(0b0000_0001);
    pub const INTERNAL: Self = Self(0b0000_0010);
    pub const CONFIDENTIAL: Self = Self(0b0000_0100);
    pub const RESTRICTED: Self = Self(0b0000_1000);

    /// Check if this tag grants access to a principal with the given role mask.
    pub fn grants(&self, role_mask: u32) -> bool {
        (self.0 & role_mask) != 0
    }

    /// Combine tags (union).
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

// ─── Node Status ────────────────────────────────────────────

/// Lifecycle status of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum NodeStatus {
    /// Node is active and valid.
    #[default]
    Active,
    /// Node has been retracted (soft delete).
    /// Retracted nodes are never deleted — they support "as of" queries.
    Retracted,
    /// Node is pending verification.
    Pending,
}

// ─── Node (七元组) ──────────────────────────────────────────

/// The core Factum knowledge node — a seven-tuple plus dependencies.
///
/// Every piece of knowledge in Factum is a Node. The seven fields are:
/// 1. `id` — unique identifier
/// 2. `predicate` — content (what is being asserted)
/// 3. `validity` — temporal validity window
/// 4. `provenance` — audit trail (where this came from)
/// 5. `confidence` — numeric confidence [0,1]
/// 6. `authority` — source authority weight [0,1]
/// 7. `permissions` — access control tag
///
/// Plus `deps` — the derivation dependency chain for invalidation.
/// When an upstream node is retracted, all `Derived` nodes transitively
/// depending on it are cascade-invalidated.
///
/// Plus `note` — an optional human-readable context string. This field
/// does not participate in content hashing or canonical equality; it
/// exists purely for human/agent context (e.g., "this is the v6 plan").
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub predicate: Predicate,
    pub validity: Validity,
    pub provenance: Provenance,
    pub confidence: Confidence,
    pub authority: Authority,
    pub permissions: PermissionTag,
    /// Derivation dependencies. When any node in this list is retracted,
    /// this node is cascade-retracted (if its provenance is Derived).
    pub deps: Vec<NodeId>,
    /// Lifecycle status.
    pub status: NodeStatus,
    /// Optional human-readable note. Does not affect content hash or equality.
    /// Useful for attaching context like "this is the v6 plan" or
    /// "extracted from page 3 of the Q4 report".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<SmolStr>,
}

impl Node {
    /// Create a new node with default metadata.
    pub fn new(id: impl Into<String>, predicate: Predicate) -> Self {
        Self {
            id: NodeId::new(id.into()),
            predicate,
            validity: Validity::default(),
            provenance: Provenance::default(),
            confidence: Confidence::default(),
            authority: Authority::default(),
            permissions: PermissionTag::PUBLIC,
            deps: Vec::new(),
            status: NodeStatus::Active,
            note: None,
        }
    }

    /// Builder: set validity.
    pub fn with_validity(mut self, v: Validity) -> Self {
        self.validity = v;
        self
    }

    /// Builder: set provenance.
    pub fn with_provenance(mut self, p: Provenance) -> Self {
        self.provenance = p;
        self
    }

    /// Builder: set confidence.
    pub fn with_confidence(mut self, c: Confidence) -> Self {
        self.confidence = c;
        self
    }

    /// Builder: set authority.
    pub fn with_authority(mut self, a: Authority) -> Self {
        self.authority = a;
        self
    }

    /// Builder: set permissions.
    pub fn with_permissions(mut self, p: PermissionTag) -> Self {
        self.permissions = p;
        self
    }

    /// Builder: add a dependency.
    pub fn with_dep(mut self, dep: NodeId) -> Self {
        self.deps.push(dep);
        self
    }

    /// Builder: set a human-readable note.
    pub fn with_note(mut self, note: impl AsRef<str>) -> Self {
        self.note = Some(SmolStr::new(note));
        self
    }

    /// Check if this node is currently active and valid.
    pub fn is_active_valid(&self) -> bool {
        self.status == NodeStatus::Active && self.validity.is_now()
    }

    /// Check if this node is active and valid at a specific time.
    pub fn is_active_valid_at(&self, t: DateTime<Utc>) -> bool {
        self.status == NodeStatus::Active && self.validity.is_valid_at(t)
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.predicate == other.predicate
            && self.validity == other.validity
            && self.provenance == other.provenance
            && self.confidence == other.confidence
            && self.authority == other.authority
            && self.permissions == other.permissions
            && self.deps == other.deps
    }
}

// ─── Errors ─────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("confidence out of range [0,1]: {0}")]
    ConfidenceOutOfRange(f32),
    #[error("authority out of range [0,1]: {0}")]
    AuthorityOutOfRange(f32),
    #[error("invalid decimal literal: {0}")]
    InvalidDecimal(String),
    #[error("decimal overflow: value exceeds i128 range")]
    DecimalOverflow,
    #[error("morpheme not found: {0}")]
    MorphemeNotFound(String),
    #[error("provenance Extracted must carry model reference")]
    MissingModelRef,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_from_str() {
        assert_eq!(Literal::dec_from_str("230.50").unwrap(), Literal::Dec(23050, 2));
        assert_eq!(Literal::dec_from_str("0").unwrap(), Literal::Dec(0, 0));
        assert_eq!(Literal::dec_from_str("-5.25").unwrap(), Literal::Dec(-525, 2));
        assert_eq!(Literal::dec_from_str("100").unwrap(), Literal::Dec(100, 0));
        assert_eq!(Literal::dec_from_str("0.001").unwrap(), Literal::Dec(1, 3));
    }

    #[test]
    fn test_decimal_canonical() {
        assert_eq!(Literal::dec_from_str("230.50").unwrap().to_canonical_string(), "230.50");
        assert_eq!(Literal::dec_from_str("0").unwrap().to_canonical_string(), "0");
        assert_eq!(Literal::dec_from_str("-5.25").unwrap().to_canonical_string(), "-5.25");
        assert_eq!(Literal::dec_from_str("0.001").unwrap().to_canonical_string(), "0.001");
    }

    #[test]
    fn test_validity() {
        let now = Utc::now();
        let past = now - chrono::Duration::hours(1);
        let future = now + chrono::Duration::hours(1);

        assert!(Validity::Forever.is_valid_at(now));

        let w = Validity::Window { from: past, until: Some(future) };
        assert!(w.is_valid_at(now));
        assert!(!w.is_valid_at(future + chrono::Duration::hours(1)));

        let open = Validity::Window { from: past, until: None };
        assert!(open.is_valid_at(now));
        assert!(open.is_valid_at(future));
    }

    #[test]
    fn test_permission_tag() {
        let tag = PermissionTag::PUBLIC.union(PermissionTag::INTERNAL);
        assert!(tag.grants(0b0000_0001)); // public access
        assert!(tag.grants(0b0000_0010)); // internal access
        assert!(!tag.grants(0b0000_0100)); // no confidential
    }

    #[test]
    fn test_node_builder() {
        let pred = Predicate::new("shareholder-major")
            .with_args(vec![Term::ent("ACME-CORP"), Term::var("p")]);
        let node = Node::new("n001", pred)
            .with_confidence(Confidence(0.85))
            .with_authority(Authority(0.9))
            .with_permissions(PermissionTag::CONFIDENTIAL);

        assert_eq!(node.confidence, Confidence(0.85));
        assert_eq!(node.authority, Authority(0.9));
        assert_eq!(node.permissions, PermissionTag::CONFIDENTIAL);
        assert!(node.is_active_valid());
    }
}
