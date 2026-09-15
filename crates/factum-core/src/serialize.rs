//! Serialization for Factum nodes.
//!
//! ## Canonical Form
//! Fields in fixed order, Dec uses minimal representation.
//! Hashing and round-trip testing use this form.
//!
//! ## Compact Form
//! Morpheme names → u32 indices, field names → 1-byte tags.
//! ~60-70% byte savings. Used for MCP transport.

use crate::types::*;
use crate::morphemes::MorphemeRegistry;
use std::sync::Arc;

/// Serializer configuration.
pub struct Serializer {
    /// If Some(registry), resolve morpheme names to IDs for compact form.
    /// Reserved for future use — compact() currently takes registry as a parameter.
    #[allow(dead_code)]
    registry: Option<Arc<MorphemeRegistry>>,
}

impl Serializer {
    pub fn new() -> Self {
        Self { registry: None }
    }

    pub fn with_registry(registry: Arc<MorphemeRegistry>) -> Self {
        Self { registry: Some(registry) }
    }
}

impl Default for Serializer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Canonical Serialization ────────────────────────────────

/// Serialize a node to canonical S-expression form.
///
/// Fields are in fixed order: pred, valid, src, conf, auth, perm, deps.
/// Dec values use minimal representation.
pub fn canonical(node: &Node) -> String {
    let mut out = String::with_capacity(256);
    canonical_node(node, &mut out);
    out
}

/// Serialize a predicate to canonical S-expression form (public).
pub fn canonical_predicate(pred: &Predicate) -> String {
    let mut out = String::with_capacity(128);
    canonical_predicate_inner(pred, &mut out);
    out
}

/// Serialize multiple nodes to canonical form.
pub fn canonical_all(nodes: &[Node]) -> String {
    let mut out = String::with_capacity(256 * nodes.len());
    for node in nodes {
        canonical_node(node, &mut out);
        out.push('\n');
    }
    out
}

fn canonical_node(node: &Node, out: &mut String) {
    out.push_str("(node ");
    out.push_str(node.id.as_str());

    // :pred
    out.push_str(" :pred ");
    canonical_predicate_inner(&node.predicate, out);

    // :valid
    out.push_str(" :valid ");
    canonical_validity(&node.validity, out);

    // :src
    out.push_str(" :src ");
    canonical_provenance(&node.provenance, out);

    // :conf
    out.push_str(" :conf ");
    let _ = std::fmt::Write::write_fmt(out, format_args!("{}", node.confidence.0));

    // :auth
    out.push_str(" :auth ");
    let _ = std::fmt::Write::write_fmt(out, format_args!("{}", node.authority.0));

    // :perm
    out.push_str(" :perm ");
    let perm_str = match node.permissions {
        PermissionTag(0b0000_0001) => "public",
        PermissionTag(0b0000_0010) => "internal",
        PermissionTag(0b0000_0100) => "confidential",
        PermissionTag(0b0000_1000) => "restricted",
        _ => "custom",
    };
    out.push_str(perm_str);

    // :deps
    if !node.deps.is_empty() {
        out.push_str(" :deps [");
        for (i, dep) in node.deps.iter().enumerate() {
            if i > 0 { out.push(' '); }
            out.push_str(dep.as_str());
        }
        out.push(']');
    }

    out.push(')');
}

fn canonical_predicate_inner(pred: &Predicate, out: &mut String) {
    out.push('(');
    match &pred.head {
        PredicateHead::Id(id) => {
            let _ = std::fmt::Write::write_fmt(out, format_args!("M{}", id.0));
        }
        PredicateHead::Name(name) => {
            out.push_str(name);
        }
    }

    for arg in &pred.args {
        out.push(' ');
        canonical_term(arg, out);
    }

    for (key, val) in &pred.named {
        out.push_str(" :");
        out.push_str(key);
        out.push(' ');
        canonical_term(val, out);
    }

    out.push(')');
}

fn canonical_term(term: &Term, out: &mut String) {
    match term {
        Term::Var(s) => {
            out.push('?');
            out.push_str(s);
        }
        Term::Ent(e) => {
            out.push('@');
            out.push_str(e.as_str());
        }
        Term::Lit(l) => {
            canonical_literal(l, out);
        }
        Term::Compound(pred) => {
            canonical_predicate_inner(pred, out);
        }
        Term::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 { out.push(' '); }
                canonical_term(item, out);
            }
            out.push(']');
        }
    }
}

fn canonical_literal(lit: &Literal, out: &mut String) {
    match lit {
        Literal::Dec(_, _) => {
            out.push_str(&lit.to_canonical_string());
        }
        Literal::Str(s) => {
            out.push('"');
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    _ => out.push(c),
                }
            }
            out.push('"');
        }
        Literal::Date(d) => {
            let _ = std::fmt::Write::write_fmt(out, format_args!("#date({})", d.format("%Y-%m-%d")));
        }
        Literal::Dur(d) => {
            let _ = std::fmt::Write::write_fmt(out, format_args!("#dur({}ns)", d.as_nanos()));
        }
        Literal::Bool(b) => {
            out.push_str(if *b { "#t" } else { "#f" });
        }
        Literal::Uri(u) => {
            out.push('<');
            out.push_str(u);
            out.push('>');
        }
    }
}

fn canonical_validity(v: &Validity, out: &mut String) {
    match v {
        Validity::Forever => out.push_str("forever"),
        Validity::Window { from, until } => {
            out.push_str("(window ");
            out.push('"');
            out.push_str(&from.to_rfc3339());
            out.push('"');
            if let Some(u) = until {
                out.push(' ');
                out.push('"');
                out.push_str(&u.to_rfc3339());
                out.push('"');
            }
            out.push(')');
        }
    }
}

fn canonical_provenance(p: &Provenance, out: &mut String) {
    match p {
        Provenance::Verbatim { doc, span } => {
            let _ = std::fmt::Write::write_fmt(out, format_args!("(verbatim \"{}\" [{} {}])", doc.0, span.start, span.end));
        }
        Provenance::Summary { doc, span } => {
            let _ = std::fmt::Write::write_fmt(out, format_args!("(summary \"{}\" [{} {}])", doc.0, span.start, span.end));
        }
        Provenance::Extracted { doc, span, model } => {
            let _ = std::fmt::Write::write_fmt(out,
                format_args!("(extracted \"{}\" [{} {}] (model \"{}\" \"{}\"))",
                    doc.0, span.start, span.end, model.name, model.version));
        }
        Provenance::Derived { from, rule } => {
            let _ = std::fmt::Write::write_fmt(out, format_args!("(derived {} \"{}\")", from, rule.0));
        }
        Provenance::Asserted { by } => {
            let _ = std::fmt::Write::write_fmt(out, format_args!("(asserted \"{}\")", by.0));
        }
    }
}

// ─── Compact Serialization (JSON-based) ─────────────────────

/// Compact JSON representation for MCP transport.
/// Uses numeric tags instead of field names.
///
/// Tag map:
/// 0: id, 1: head, 2: args, 3: named, 4: valid, 5: src,
/// 6: conf, 7: auth, 8: perm, 9: deps
pub fn compact(node: &Node, registry: &MorphemeRegistry) -> String {
    serde_json::to_string(&CompactNode::from_node(node, registry))
        .unwrap_or_else(|_| "{}".to_string())
}

/// Serialize multiple nodes in compact form.
pub fn compact_all(nodes: &[Node], registry: &MorphemeRegistry) -> String {
    let compact: Vec<_> = nodes.iter()
        .map(|n| CompactNode::from_node(n, registry))
        .collect();
    serde_json::to_string(&compact).unwrap_or_else(|_| "[]".to_string())
}

/// Compact node representation for JSON serialization.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CompactNode {
    #[serde(rename = "0")]
    pub id: String,
    #[serde(rename = "1")]
    pub head: CompactHead,
    #[serde(rename = "2")]
    pub args: Vec<CompactTerm>,
    #[serde(rename = "3", skip_serializing_if = "Vec::is_empty", default)]
    pub named: Vec<(String, CompactTerm)>,
    #[serde(rename = "4", default = "default_valid")]
    pub valid: CompactValidity,
    #[serde(rename = "5")]
    pub src: CompactProvenance,
    #[serde(rename = "6")]
    pub conf: f32,
    #[serde(rename = "7")]
    pub auth: f32,
    #[serde(rename = "8", default = "default_perm")]
    pub perm: u32,
    #[serde(rename = "9", skip_serializing_if = "Vec::is_empty", default)]
    pub deps: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum CompactHead {
    Name(String),
    Id(u32),
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum CompactTerm {
    Var(String),
    Ent(String),
    Lit(String), // Encoded literal: "230.50@dec", "\"hello\"", "#date(2024-01-01)", etc.
    List(Vec<CompactTerm>),
    Compound(Box<CompactPredicate>),
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CompactPredicate {
    pub h: CompactHead,
    pub a: Vec<CompactTerm>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub n: Vec<(String, CompactTerm)>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub enum CompactValidity {
    #[default]
    Forever,
    Window { f: String, u: Option<String> },
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", content = "v")]
pub enum CompactProvenance {
    Verbatim { d: String, s: [u32; 2] },
    Summary { d: String, s: [u32; 2] },
    Extracted { d: String, s: [u32; 2], m: [String; 2] }, // [name, version]
    Derived { f: String, r: String },
    Asserted { b: String },
}

fn default_valid() -> CompactValidity { CompactValidity::Forever }
fn default_perm() -> u32 { 1 }

impl CompactNode {
    pub fn from_node(node: &Node, registry: &MorphemeRegistry) -> Self {
        let head = match &node.predicate.head {
            PredicateHead::Name(name) => {
                if let Some(id) = registry.resolve(name) {
                    CompactHead::Id(id.0)
                } else {
                    CompactHead::Name(name.to_string())
                }
            }
            PredicateHead::Id(id) => CompactHead::Id(id.0),
        };

        let args: Vec<_> = node.predicate.args.iter()
            .map(|t| CompactTerm::from_term(t, registry))
            .collect();

        let named: Vec<_> = node.predicate.named.iter()
            .map(|(k, v)| (k.to_string(), CompactTerm::from_term(v, registry)))
            .collect();

        let valid = match &node.validity {
            Validity::Forever => CompactValidity::Forever,
            Validity::Window { from, until } => CompactValidity::Window {
                f: from.format("%Y-%m-%d").to_string(),
                u: until.map(|u| u.format("%Y-%m-%d").to_string()),
            },
        };

        let src = match &node.provenance {
            Provenance::Verbatim { doc, span } => CompactProvenance::Verbatim {
                d: doc.0.to_string(), s: [span.start, span.end],
            },
            Provenance::Summary { doc, span } => CompactProvenance::Summary {
                d: doc.0.to_string(), s: [span.start, span.end],
            },
            Provenance::Extracted { doc, span, model } => CompactProvenance::Extracted {
                d: doc.0.to_string(), s: [span.start, span.end],
                m: [model.name.to_string(), model.version.to_string()],
            },
            Provenance::Derived { from, rule } => CompactProvenance::Derived {
                f: from.to_string(), r: rule.0.to_string(),
            },
            Provenance::Asserted { by } => CompactProvenance::Asserted {
                b: by.0.to_string(),
            },
        };

        Self {
            id: node.id.to_string(),
            head,
            args,
            named,
            valid,
            src,
            conf: node.confidence.0,
            auth: node.authority.0,
            perm: node.permissions.0,
            deps: node.deps.iter().map(|d| d.to_string()).collect(),
        }
    }
}

impl CompactTerm {
    pub fn from_term(term: &Term, _registry: &MorphemeRegistry) -> Self {
        match term {
            Term::Var(s) => CompactTerm::Var(s.to_string()),
            Term::Ent(e) => CompactTerm::Ent(e.to_string()),
            Term::Lit(l) => {
                CompactTerm::Lit(l.to_canonical_string())
            }
            Term::List(items) => {
                CompactTerm::List(items.iter()
                    .map(|t| CompactTerm::from_term(t, _registry))
                    .collect())
            }
            Term::Compound(pred) => {
                let head = match &pred.head {
                    PredicateHead::Name(n) => CompactHead::Name(n.to_string()),
                    PredicateHead::Id(id) => CompactHead::Id(id.0),
                };
                CompactTerm::Compound(Box::new(CompactPredicate {
                    h: head,
                    a: pred.args.iter().map(|t| CompactTerm::from_term(t, _registry)).collect(),
                    n: pred.named.iter().map(|(k, v)| (k.to_string(), CompactTerm::from_term(v, _registry))).collect(),
                }))
            }
        }
    }
}

// ─── Round-trip testing ─────────────────────────────────────

/// Verify that parse(canonical(node)) == node.
/// This is the fundamental round-trip invariant.
pub fn verify_roundtrip(node: &Node) -> Result<(), String> {
    let serialized = canonical(node);
    let parsed = crate::parser::Parser::parse(&serialized)
        .map_err(|e| format!("Parse error during roundtrip: {}", e))?;

    if parsed.len() != 1 {
        return Err(format!("Expected 1 node, got {}", parsed.len()));
    }

    let reparsed = &parsed[0];
    if node != reparsed {
        return Err(format!(
            "Roundtrip mismatch:\n  Original: {:?}\n  Reparsed: {:?}\n  Serialized: {}",
            node, reparsed, serialized
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morphemes::MorphemeRegistry;
    use smol_str::SmolStr;
    use chrono::Utc;

    fn make_test_node() -> Node {
        Node::new("n001",
            crate::types::Predicate::new("shareholder-major")
                .with_args(vec![
                    Term::ent("ACME-CORP"),
                    Term::ent("FOUNDER-1"),
                    Term::lit(Literal::dec_from_str("0.73").unwrap()),
                ]))
            .with_confidence(Confidence(0.85))
            .with_authority(Authority(0.9))
            .with_permissions(PermissionTag::CONFIDENTIAL)
    }

    #[test]
    fn test_canonical_serialization() {
        let node = make_test_node();
        let s = canonical(&node);
        assert!(s.contains("(node n001"));
        assert!(s.contains("shareholder-major"));
        assert!(s.contains("@ACME-CORP"));
        assert!(s.contains("0.73"));
        assert!(s.contains(":conf 0.85"));
        assert!(s.contains(":perm confidential"));
    }

    #[test]
    fn test_roundtrip_basic() {
        let node = make_test_node();
        verify_roundtrip(&node).expect("roundtrip should succeed");
    }

    #[test]
    fn test_roundtrip_with_validity() {
        let now = Utc::now();
        let node = Node::new("n002",
            Predicate::new("ceo-of")
                .with_args(vec![Term::ent("X"), Term::ent("Y")]))
            .with_validity(Validity::Window {
                from: now - chrono::Duration::days(1),
                until: Some(now + chrono::Duration::days(1)),
            });
        verify_roundtrip(&node).expect("roundtrip should succeed");
    }

    #[test]
    fn test_roundtrip_with_provenance() {
        let node = Node::new("n003",
            Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")]))
            .with_provenance(Provenance::Extracted {
                doc: DocId::new("doc001"),
                span: Span { start: 0, end: 100 },
                model: ModelRef {
                    name: SmolStr::new("gpt-4"),
                    version: SmolStr::new("2024-06"),
                },
            });
        verify_roundtrip(&node).expect("roundtrip should succeed");
    }

    #[test]
    fn test_roundtrip_with_deps() {
        let node = Node::new("n004",
            Predicate::new("subsidiary-of")
                .with_args(vec![Term::ent("X"), Term::ent("Y")]))
            .with_dep(NodeId::new("n001"))
            .with_dep(NodeId::new("n002"));
        verify_roundtrip(&node).expect("roundtrip should succeed");
    }

    #[test]
    fn test_roundtrip_with_named_args() {
        let node = Node::new("n005",
            Predicate::new("revenue")
                .with_args(vec![Term::ent("X"), Term::lit(Literal::dec_from_str("1000").unwrap())])
                .with_named("period", Term::lit(Literal::Date(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())))
                .with_named("currency", Term::lit(Literal::Str(SmolStr::new("CNY")))));
        verify_roundtrip(&node).expect("roundtrip should succeed");
    }

    #[test]
    fn test_compact_serialization() {
        let registry = MorphemeRegistry::with_seeds();
        let node = make_test_node();
        let s = compact(&node, &registry);
        assert!(s.contains("\"0\":\"n001\""));
        // Morpheme resolved to ID
        assert!(s.contains("\"1\":") ); // head is either name or id
    }

    #[test]
    fn test_compact_with_registry() {
        let registry = MorphemeRegistry::with_seeds();
        // shareholder-major should be resolved to its ID
        let id = registry.resolve("shareholder-major").unwrap();
        let node = make_test_node();
        let s = compact(&node, &registry);
        assert!(s.contains(&id.0.to_string()));
    }
}
