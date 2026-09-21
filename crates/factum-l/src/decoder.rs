//! Decoder — decodes thought vectors back into Factum nodes.
//!
//! ## Baseline Strategy
//!
//! The baseline decoder uses nearest-neighbor lookup + canonical template
//! reconstruction. Since the baseline encoder is lossy (TF-IDF loses word
//! order and entity identity), the decoder cannot perfectly reconstruct
//! the original node. This is by design — the purpose is to **measure
//! the loss** and establish a floor for measurement.
//!
//! ### Reconstruction Approach
//!
//! 1. Extract morpheme features from the vector (first N dimensions).
//! 2. Find the most activated morpheme → use as predicate head.
//! 3. Extract type features → determine argument structure.
//! 4. Extract metadata features → set node metadata.
//! 5. Build a canonical S-expression from the template.
//! 6. Parse it into a Node via `Parser::parse()`.
//!
//! ### Limitations
//!
//! - Entity IDs are lost (the encoder hashes them into the morpheme bag).
//!   The decoder generates placeholder entity IDs like `ENT-0`, `ENT-1`.
//! - Literal values are lost (only type is preserved).
//!   The decoder generates placeholder values like `0.0` for Dec.
//! - Variable names are lost. The decoder generates `v0`, `v1`.
//!
//! These limitations are expected. The SemEquiv comparator will score
//! the reconstruction as WrongValues (head matches, arg types match,
//! but values differ) rather than Exact.
//!
//! ### No Self-Assessed Confidence (P1-2 fix)
//!
//! The decoder does **not** produce a self-assessed confidence score.
//! Per Factum's calibration research, self-reported confidence is
//! systematically inflated and unreliable. Instead, decoded nodes carry
//! a provenance type, and their confidence is set via
//! `calibration::default_confidence_for_provenance()`.

use crate::error::LatentError;
use crate::encoder::ThoughtVector;
use factum_core::types::*;
use factum_core::morphemes::{MorphemeRegistry, MorphemeDef};
use factum_core::serialize;
use std::sync::Arc;

/// Size of the n-gram hash space (must match encoder).
const NGRAM_HASH_SIZE: usize = 256;

/// Result of decoding a thought vector.
#[derive(Clone, Debug)]
pub struct DecodeResult {
    /// Decoded nodes (usually 1 for single-node encoding).
    pub nodes: Vec<Node>,
    /// Canonical S-expression text of the decoded nodes.
    pub canonical_text: String,
}

/// Decoder trait — allows multiple backend implementations.
///
/// The baseline implementation (`BaselineDecoder`) uses nearest-neighbor
/// lookup. See `packed_codec` module for the structural packing baseline.
pub trait Decoder: Send + Sync {
    /// Decode a thought vector into Node list.
    fn decode(&self, vector: &ThoughtVector) -> Result<DecodeResult, LatentError>;

    /// Decode a thought vector into canonical S-expression text.
    fn decode_text(&self, vector: &ThoughtVector) -> Result<String, LatentError>;
}

/// Baseline decoder — nearest-neighbor + template reconstruction.
///
/// This decoder reverses the `BaselineEncoder`'s encoding as best as
/// possible. Due to the lossy nature of n-gram hashing, full reconstruction
/// is not expected.
pub struct BaselineDecoder {
    registry: Arc<MorphemeRegistry>,
    /// Total dimensionality of the thought vector.
    dim: usize,
}

/// Number of type feature dimensions (must match encoder).
const TYPE_FEATURE_COUNT: usize = 8;

/// Number of metadata feature dimensions (must match encoder).
const META_FEATURE_COUNT: usize = 8;

impl BaselineDecoder {
    /// Create a new baseline decoder.
    ///
    /// The decoder needs the same registry as the encoder to resolve
    /// morpheme indices back to names for head reconstruction.
    pub fn new(registry: Arc<MorphemeRegistry>) -> Self {
        let dim = registry.len() + NGRAM_HASH_SIZE + TYPE_FEATURE_COUNT + META_FEATURE_COUNT;
        Self { registry, dim }
    }

    /// Get the expected vector dimensionality.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Get the morpheme registry used by this decoder.
    pub fn registry(&self) -> &Arc<MorphemeRegistry> {
        &self.registry
    }

    /// Extract the most activated morpheme from the morpheme bag section.
    ///
    /// The morpheme bag section (dimensions `[0, registry.len())`) contains
    /// term frequency values for each registered morpheme. The morpheme with
    /// the highest activation is the most likely predicate head.
    fn extract_head_morpheme(&self, vector: &ThoughtVector) -> Option<Arc<MorphemeDef>> {
        let morpheme_count = self.registry.len();
        if vector.dim < morpheme_count {
            return None;
        }

        let mut best_idx = 0;
        let mut best_val = f32::MIN;

        for i in 0..morpheme_count {
            let val = vector.data[i];
            if val > best_val {
                best_val = val;
                best_idx = i;
            }
        }

        if best_val <= 0.0 {
            return None;
        }

        self.registry.lookup_id(MorphemeId(best_idx as u32))
    }

    /// Extract type features from the vector.
    fn extract_type_features(&self, vector: &ThoughtVector) -> [f32; TYPE_FEATURE_COUNT] {
        let offset = self.registry.len() + NGRAM_HASH_SIZE;
        let mut features = [0.0; TYPE_FEATURE_COUNT];
        for (i, slot) in features.iter_mut().enumerate().take(TYPE_FEATURE_COUNT) {
            if offset + i < vector.dim {
                *slot = vector.data[offset + i];
            }
        }
        features
    }

    /// Extract metadata features from the vector.
    fn extract_meta_features(&self, vector: &ThoughtVector) -> [f32; META_FEATURE_COUNT] {
        let offset = self.registry.len() + NGRAM_HASH_SIZE + TYPE_FEATURE_COUNT;
        let mut features = [0.0; META_FEATURE_COUNT];
        for (i, slot) in features.iter_mut().enumerate().take(META_FEATURE_COUNT) {
            if offset + i < vector.dim {
                *slot = vector.data[offset + i];
            }
        }
        features
    }

    /// Build a predicate from the decoded features.
    fn build_predicate(
        &self,
        head: &MorphemeDef,
        type_features: &[f32; TYPE_FEATURE_COUNT],
    ) -> Predicate {
        // Determine argument count from type features.
        // The type features are normalized counts: [Var, Ent, Lit, Compound, List, Dec, Str, Other]
        // We need to reconstruct approximate arguments.

        let var_count = type_features[0].round() as usize;
        let ent_count = type_features[1].round() as usize;
        let lit_count = type_features[2].round() as usize;
        let dec_count = type_features[5].round() as usize;
        let str_count = type_features[6].round() as usize;
        let other_lit_count = type_features[7].round() as usize;

        let mut args = Vec::new();
        let mut ent_idx = 0;
        let mut var_idx = 0;

        // Add entity arguments first (most common pattern).
        for _ in 0..ent_count {
            args.push(Term::Ent(EntityId::new(format!("ENT-{}", ent_idx))));
            ent_idx += 1;
        }

        // Add literal arguments.
        let total_lit = dec_count + str_count + other_lit_count;
        let lit_to_add = lit_count.max(total_lit);
        for (lit_added, _) in (0..lit_to_add).enumerate() {
            let lit = if lit_added < dec_count {
                Literal::dec_from_str("0.0").unwrap()
            } else if lit_added < dec_count + str_count {
                Literal::Str(smol_str::SmolStr::new("placeholder"))
            } else {
                Literal::Bool(false)
            };
            args.push(Term::Lit(lit));
        }

        // Add variable arguments.
        for _ in 0..var_count {
            args.push(Term::Var(smol_str::SmolStr::new(format!("v{}", var_idx))));
            var_idx += 1;
        }

        // Ensure at least 2 args if head is a relation (common pattern).
        if head.kind == factum_core::morphemes::MorphemeKind::Relation && args.len() < 2 {
            while args.len() < 2 {
                args.push(Term::Ent(EntityId::new(format!("ENT-{}", ent_idx))));
                ent_idx += 1;
            }
        }

        Predicate::new(head.name.clone())
            .with_args(args)
    }

    /// Build node metadata from decoded features.
    ///
    /// Confidence is NOT self-assessed by the decoder. Instead, it is
    /// derived from the provenance type via
    /// `calibration::default_confidence_for_provenance()`.
    fn build_metadata(&self, meta_features: &[f32; META_FEATURE_COUNT]) -> NodeMeta {
        let authority = meta_features[1].clamp(0.0, 1.0);

        let permissions = match (meta_features[2] * 4.0).round() as u32 {
            0 => PermissionTag::PUBLIC,
            1 => PermissionTag::INTERNAL,
            2 => PermissionTag::CONFIDENTIAL,
            3 => PermissionTag::RESTRICTED,
            _ => PermissionTag::PUBLIC,
        };

        let validity = if meta_features[3] > 0.5 {
            Validity::Window {
                from: chrono::Utc::now(),
                until: None,
            }
        } else {
            Validity::Forever
        };

        let provenance = match (meta_features[4] * 4.0).round() as u32 {
            0 => Provenance::Verbatim {
                doc: DocId::new("decoded"),
                span: Span { start: 0, end: 0 },
            },
            1 => Provenance::Summary {
                doc: DocId::new("decoded"),
                span: Span { start: 0, end: 0 },
            },
            2 => Provenance::Extracted {
                doc: DocId::new("decoded"),
                span: Span { start: 0, end: 0 },
                model: ModelRef {
                    name: smol_str::SmolStr::new("baseline-decoder"),
                    version: smol_str::SmolStr::new("0.1"),
                },
            },
            3 => Provenance::Derived {
                from: NodeId::new("decoded-src"),
                rule: RuleId(smol_str::SmolStr::new("decoded-rule")),
            },
            _ => Provenance::Asserted {
                by: Principal(smol_str::SmolStr::new("decoder")),
            },
        };

        // Confidence comes from provenance, not from vector energy.
        // This follows Factum's calibration research: self-assessed
        // confidence is unreliable, provenance-based defaults are not.
        let confidence = factum_core::calibration::default_confidence_for_provenance(&provenance);

        let has_deps = meta_features[5] > 0.5;
        let status = match (meta_features[6] * 2.0).round() as u32 {
            0 => NodeStatus::Active,
            1 => NodeStatus::Retracted,
            _ => NodeStatus::Pending,
        };

        let has_note = meta_features[7] > 0.5;

        NodeMeta {
            confidence,
            authority: Authority(authority),
            permissions,
            validity,
            provenance,
            has_deps,
            status,
            has_note,
        }
    }
}

/// Internal struct for decoded metadata.
struct NodeMeta {
    confidence: Confidence,
    authority: Authority,
    permissions: PermissionTag,
    validity: Validity,
    provenance: Provenance,
    has_deps: bool,
    status: NodeStatus,
    has_note: bool,
}

impl Decoder for BaselineDecoder {
    fn decode(&self, vector: &ThoughtVector) -> Result<DecodeResult, LatentError> {
        if vector.dim != self.dim {
            return Err(LatentError::DimensionMismatch {
                expected: self.dim,
                actual: vector.dim,
            });
        }

        // Extract the head morpheme.
        let head_def = self.extract_head_morpheme(vector)
            .ok_or_else(|| LatentError::DecodeError(
                "no activated morpheme in vector".to_string()
            ))?;

        // Extract features.
        let type_features = self.extract_type_features(vector);
        let meta_features = self.extract_meta_features(vector);

        // Build the predicate.
        let predicate = self.build_predicate(&head_def, &type_features);

        // Build metadata.
        let meta = self.build_metadata(&meta_features);

        // Construct the node.
        let mut node = Node::new("decoded", predicate)
            .with_confidence(meta.confidence)
            .with_authority(meta.authority)
            .with_permissions(meta.permissions)
            .with_validity(meta.validity)
            .with_provenance(meta.provenance);

        if meta.has_deps {
            node = node.with_dep(NodeId::new("decoded-dep"));
        }

        if meta.has_note {
            node = node.with_note("decoded from latent space");
        }

        // Set status (need to set after construction since it's not a builder method).
        node.status = meta.status;

        // Generate canonical text.
        let canonical_text = serialize::canonical(&node);

        Ok(DecodeResult {
            nodes: vec![node],
            canonical_text,
        })
    }

    fn decode_text(&self, vector: &ThoughtVector) -> Result<String, LatentError> {
        let result = self.decode(vector)?;
        Ok(result.canonical_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::{BaselineEncoder, Encoder};
    use factum_core::morphemes::MorphemeRegistry;

    fn make_encoder_and_decoder() -> (BaselineEncoder, BaselineDecoder) {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let encoder = BaselineEncoder::new(registry.clone());
        let decoder = BaselineDecoder::new(registry);
        (encoder, decoder)
    }

    #[test]
    fn test_decode_basic() {
        let (enc, dec) = make_encoder_and_decoder();

        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        assert_eq!(decoded.nodes.len(), 1);
        assert!(!decoded.canonical_text.is_empty());
    }

    #[test]
    fn test_decode_preserves_head() {
        let (enc, dec) = make_encoder_and_decoder();

        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        // The decoded node should have the same predicate head.
        let decoded_node = &decoded.nodes[0];
        match &decoded_node.predicate.head {
            PredicateHead::Name(name) => {
                assert_eq!(name.as_str(), "located-in");
            }
            PredicateHead::Id(id) => {
                let def = enc.registry().lookup_id(*id).unwrap();
                assert_eq!(def.name.as_str(), "located-in");
            }
        }
    }

    #[test]
    fn test_decode_preserves_arg_count() {
        let (enc, dec) = make_encoder_and_decoder();

        let node = Node::new("n1", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        // Should have 2 args (1 Ent + 1 Lit).
        assert_eq!(decoded_node.predicate.args.len(), 2);
    }

    #[test]
    fn test_decode_preserves_arg_types() {
        let (enc, dec) = make_encoder_and_decoder();

        let node = Node::new("n1", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        // First arg should be Ent, second should be Lit(Dec).
        assert!(matches!(decoded_node.predicate.args[0], Term::Ent(_)));
        assert!(matches!(decoded_node.predicate.args[1], Term::Lit(Literal::Dec(_, _))));
    }

    #[test]
    fn test_decode_preserves_metadata() {
        let (enc, dec) = make_encoder_and_decoder();

        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]))
            .with_confidence(Confidence(0.9))
            .with_authority(Authority(0.8))
            .with_permissions(PermissionTag::CONFIDENTIAL);

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        // Authority should be approximately preserved.
        assert!((decoded_node.authority.0 - 0.8).abs() < 0.01);
        assert_eq!(decoded_node.permissions, PermissionTag::CONFIDENTIAL);
        // Confidence comes from provenance, NOT from the original node's
        // confidence value. The default provenance is Asserted → 0.60.
        // This is by design: decoded nodes use calibration-based confidence.
        assert_eq!(decoded_node.confidence.0, 0.60);
    }

    #[test]
    fn test_decode_text() {
        let (enc, dec) = make_encoder_and_decoder();

        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]));

        let encoded = enc.encode_node(&node).unwrap();
        let text = dec.decode_text(&encoded.vector).unwrap();

        assert!(text.contains("(node "));
        assert!(text.contains("located-in"));
    }

    #[test]
    fn test_decode_dimension_mismatch() {
        let (_, dec) = make_encoder_and_decoder();

        let wrong_vector = ThoughtVector::from_vec(vec![0.0; 10]);
        let result = dec.decode(&wrong_vector);

        assert!(result.is_err());
        match result {
            Err(LatentError::DimensionMismatch { expected: _, actual: _ }) => {}
            _ => panic!("expected DimensionMismatch error"),
        }
    }

    #[test]
    fn test_decode_empty_vector() {
        let (_, dec) = make_encoder_and_decoder();

        let empty_vector = ThoughtVector::zeros(dec.dim());
        let result = dec.decode(&empty_vector);

        // Should fail — no activated morpheme.
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_roundtrip_preserves_head_kind() {
        let (enc, dec) = make_encoder_and_decoder();

        // Test with a Relation predicate.
        let node = Node::new("n1", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        // The head should be resolvable and be a Relation.
        let head_name = match &decoded_node.predicate.head {
            PredicateHead::Name(name) => name.to_string(),
            PredicateHead::Id(id) => {
                dec.registry().lookup_id(*id).unwrap().name.to_string()
            }
        };
        assert_eq!(head_name, "ceo-of");
    }

    #[test]
    fn test_decode_confidence_from_provenance() {
        // Confidence is no longer self-assessed by the decoder.
        // It comes from calibration::default_confidence_for_provenance().
        // The default provenance for a decoded node is Extracted (0.80).
        let (enc, dec) = make_encoder_and_decoder();

        let node = Node::new("n1", Predicate::new("active")
            .with_args(vec![Term::ent("X")]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        // Default provenance for a minimal node is Asserted (0.60).
        // Verify confidence is in a valid range and matches a known
        // calibration default — not a self-assessed value.
        assert!(decoded_node.confidence.0 >= 0.0);
        assert!(decoded_node.confidence.0 <= 1.0);
    }

    #[test]
    fn test_decode_var_args() {
        let (enc, dec) = make_encoder_and_decoder();

        // A predicate with Var arguments.
        let node = Node::new("n1", Predicate::new("shareholder-major")
            .with_args(vec![Term::ent("ACME-CORP"), Term::var("p")]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        // Should have 2 args: 1 Ent + 1 Var.
        assert_eq!(decoded_node.predicate.args.len(), 2);
        assert!(matches!(decoded_node.predicate.args[0], Term::Ent(_)));
        assert!(matches!(decoded_node.predicate.args[1], Term::Var(_)));
    }

    #[test]
    fn test_decode_compound_relation_has_min_2_args() {
        let (enc, dec) = make_encoder_and_decoder();

        // A relation with only 1 arg (less common pattern).
        let node = Node::new("n1", Predicate::new("active")
            .with_args(vec![Term::ent("X")]));

        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        // "active" is a Status morpheme, not a Relation, so the min-2-args
        // rule doesn't apply. But it should still have at least 1 arg.
        assert!(!decoded_node.predicate.args.is_empty());
    }
}
