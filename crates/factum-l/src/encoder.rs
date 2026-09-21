//! Encoder — encodes Factum nodes into continuous thought vectors.
//!
//! ## Baseline Strategy (v2: n-gram TF-IDF + morpheme bag hybrid)
//!
//! The baseline encoder uses a **hybrid** approach:
//! 1. **Morpheme bag-of-words** (for head identification) — the decoder
//!    uses this section to reverse-lookup the predicate head morpheme.
//! 2. **Character n-gram TF-IDF** over canonical text (for argument order
//!    preservation) — this fixes P0-2: `(ceo-of @X @Y)` and
//!    `(ceo-of @Y @X)` now produce different vectors because their
//!    canonical texts differ, producing different n-gram hash distributions.
//!
//! ### Why hybrid?
//!
//! The v1 morpheme bag alone lost argument order. The n-gram hash alone
//! makes head identification unreliable due to hash collisions. The hybrid
//! approach uses each section for what it does best:
//! - Morpheme bag → head identification (decoder reverse lookup)
//! - N-gram hash → argument order and entity identity preservation
//!
//! ### Feature Space
//!
//! The thought vector has the following layout:
//! - Dimensions `[0, M)`: Morpheme bag-of-words (M = registry size).
//! - Dimensions `[M, M+N)`: Character n-gram TF-IDF (N = 256).
//!   Uses 2, 3, and 4-grams hashed into a fixed-size vector.
//! - Dimensions `[M+N, M+N+K)`: Type features (K = 8).
//! - Dimensions `[M+N+K, M+N+K+L)`: Metadata features (L = 8).
//!
//! ### Content Hash
//!
//! The `source_hash` in `EncodeResult` is computed from the canonical
//! serialization of the node. This is used for round-trip verification.

use crate::error::LatentError;
use factum_core::types::*;
use factum_core::morphemes::MorphemeRegistry;
use factum_core::serialize;
use std::sync::Arc;
use std::collections::HashMap;

/// Size of the n-gram hash space.
const NGRAM_HASH_SIZE: usize = 256;

/// N-gram sizes to extract (2, 3, and 4 character grams).
const NGRAM_SIZES: &[usize] = &[2, 3, 4];

/// Continuous thought vector.
///
/// A simple `Vec<f32>` with dimension metadata. The encoding scheme is
/// determined by the encoder implementation — see `BaselineEncoder` for
/// the baseline feature layout.
#[derive(Clone, Debug)]
pub struct ThoughtVector {
    /// The vector data. Length should equal `dim`.
    pub data: Vec<f32>,
    /// Dimensionality of the vector.
    pub dim: usize,
}

impl ThoughtVector {
    /// Create a zero vector of the given dimension.
    pub fn zeros(dim: usize) -> Self {
        Self {
            data: vec![0.0; dim],
            dim,
        }
    }

    /// Create from an existing data vector.
    pub fn from_vec(data: Vec<f32>) -> Self {
        let dim = data.len();
        Self { data, dim }
    }

    /// Get the L2 norm of the vector.
    pub fn l2_norm(&self) -> f32 {
        self.data.iter().map(|v| v * v).sum::<f32>().sqrt()
    }

    /// Normalize to unit length (L2). Returns a new vector.
    /// If the vector is all zeros, returns a copy.
    pub fn normalize(&self) -> Self {
        let norm = self.l2_norm();
        if norm < 1e-10 {
            return self.clone();
        }
        Self::from_vec(self.data.iter().map(|v| v / norm).collect())
    }

    /// Compute cosine similarity with another vector.
    /// Returns 0.0 if dimensions don't match.
    pub fn cosine_similarity(&self, other: &ThoughtVector) -> f32 {
        if self.dim != other.dim {
            return 0.0;
        }
        let dot: f32 = self.data.iter().zip(other.data.iter())
            .map(|(a, b)| a * b)
            .sum();
        let norm_a = self.l2_norm();
        let norm_b = other.l2_norm();
        if norm_a < 1e-10 || norm_b < 1e-10 {
            return 0.0;
        }
        dot / (norm_a * norm_b)
    }
}

/// Result of encoding a node or text.
#[derive(Clone, Debug)]
pub struct EncodeResult {
    /// The thought vector.
    pub vector: ThoughtVector,
    /// Content hash of the source (canonical serialization).
    /// Used for round-trip verification.
    pub source_hash: u64,
}

/// Encoder trait — allows multiple backend implementations.
///
/// The baseline implementation (`BaselineEncoder`) uses TF-IDF.
/// See `packed_codec` module for the structural packing baseline.
pub trait Encoder: Send + Sync {
    /// Encode a single node into a thought vector.
    fn encode_node(&self, node: &Node) -> Result<EncodeResult, LatentError>;

    /// Encode a sequence of nodes into a single thought vector.
    /// The encoding aggregates features from all nodes.
    fn encode_sequence(&self, nodes: &[Node]) -> Result<EncodeResult, LatentError>;

    /// Encode canonical S-expression text into a thought vector.
    /// This parses the text first, then encodes the resulting nodes.
    fn encode_text(&self, text: &str) -> Result<EncodeResult, LatentError>;
}

/// Number of type feature dimensions.
/// These encode the structural types of terms in the predicate.
const TYPE_FEATURE_COUNT: usize = 8;

/// Number of metadata feature dimensions.
const META_FEATURE_COUNT: usize = 8;

/// Baseline encoder — morpheme bag + n-gram TF-IDF, no ML dependencies.
///
/// Uses a hybrid approach:
/// - Morpheme bag-of-words for head identification (decoder needs this)
/// - Character n-grams (2, 3, 4-gram) over canonical text for argument
///   order and entity identity preservation (P0-2 fix)
/// - Type and metadata features for structural information
///
/// This encoder establishes the measurable floor. It is expected to produce
/// low semantic round-trip scores — this is by design.
pub struct BaselineEncoder {
    registry: Arc<MorphemeRegistry>,
    /// Total dimensionality of the thought vector.
    dim: usize,
}

impl BaselineEncoder {
    /// Create a new baseline encoder with the given morpheme registry.
    ///
    /// The dimensionality is determined by:
    /// `registry.len() + NGRAM_HASH_SIZE + TYPE_FEATURE_COUNT + META_FEATURE_COUNT`
    pub fn new(registry: Arc<MorphemeRegistry>) -> Self {
        let dim = registry.len() + NGRAM_HASH_SIZE + TYPE_FEATURE_COUNT + META_FEATURE_COUNT;
        Self { registry, dim }
    }

    /// Get the morpheme registry used by this encoder.
    pub fn registry(&self) -> &Arc<MorphemeRegistry> {
        &self.registry
    }

    /// Get the dimensionality of vectors produced by this encoder.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Get the offset of the n-gram section in the vector.
    pub fn ngram_offset(&self) -> usize {
        self.registry.len()
    }

    /// Get the offset of the type features section in the vector.
    pub fn type_offset(&self) -> usize {
        self.registry.len() + NGRAM_HASH_SIZE
    }

    /// Get the offset of the metadata features section in the vector.
    pub fn meta_offset(&self) -> usize {
        self.registry.len() + NGRAM_HASH_SIZE + TYPE_FEATURE_COUNT
    }

    /// Extract character n-gram features from canonical text.
    ///
    /// Uses 2, 3, and 4-character grams, hashed into a fixed-size vector.
    /// The hash function is FNV-1a to ensure deterministic, reproducible
    /// feature indices.
    ///
    /// Returns a vector of length `NGRAM_HASH_SIZE` with TF (term frequency)
    /// values. IDF is not used since we don't have a document corpus —
    /// this is a weak baseline by design.
    fn extract_ngram_features(&self, text: &str) -> Vec<f32> {
        let mut features = vec![0.0f32; NGRAM_HASH_SIZE];
        let chars: Vec<char> = text.chars().collect();

        for &n in NGRAM_SIZES {
            if chars.len() < n {
                continue;
            }
            for i in 0..=chars.len() - n {
                let gram: String = chars[i..i + n].iter().collect();
                let hash = fnv1a_hash(&gram) as usize % NGRAM_HASH_SIZE;
                features[hash] += 1.0;
            }
        }

        // Normalize by total n-gram count to keep values in a reasonable range.
        let total: f32 = features.iter().sum();
        if total > 0.0 {
            for f in features.iter_mut() {
                *f /= total;
            }
        }

        features
    }

    /// Extract morpheme features from a predicate.
    ///
    /// Returns a map from morpheme index to term frequency.
    fn extract_morpheme_features(&self, pred: &Predicate) -> HashMap<usize, f32> {
        let mut features = HashMap::new();

        // Head morpheme.
        if let Some(idx) = self.head_to_index(&pred.head) {
            *features.entry(idx).or_insert(0.0) += 1.0;
        }

        // Morphemes in compound terms (nested predicates).
        for arg in &pred.args {
            self.extract_morpheme_features_from_term(arg, &mut features);
        }
        for (_, val) in &pred.named {
            self.extract_morpheme_features_from_term(val, &mut features);
        }

        features
    }

    fn extract_morpheme_features_from_term(&self, term: &Term, features: &mut HashMap<usize, f32>) {
        match term {
            Term::Compound(pred) => {
                if let Some(idx) = self.head_to_index(&pred.head) {
                    *features.entry(idx).or_insert(0.0) += 1.0;
                }
                for arg in &pred.args {
                    self.extract_morpheme_features_from_term(arg, features);
                }
                for (_, val) in &pred.named {
                    self.extract_morpheme_features_from_term(val, features);
                }
            }
            Term::List(items) => {
                for item in items {
                    self.extract_morpheme_features_from_term(item, features);
                }
            }
            _ => {}
        }
    }

    /// Resolve a predicate head to a vector index.
    fn head_to_index(&self, head: &PredicateHead) -> Option<usize> {
        match head {
            PredicateHead::Id(id) => Some(id.0 as usize),
            PredicateHead::Name(name) => {
                self.registry.resolve(name).map(|id| id.0 as usize)
            }
        }
    }

    /// Extract type features from a predicate's arguments.
    ///
    /// Type features encode the structural types present in the arguments:
    /// - [0]: Var count
    /// - [1]: Ent count
    /// - [2]: Lit count
    /// - [3]: Compound count
    /// - [4]: List count
    /// - [5]: Literal::Dec count
    /// - [6]: Literal::Str count
    /// - [7]: Literal::Date/Bool/Uri/Dur count
    fn extract_type_features(&self, pred: &Predicate) -> [f32; TYPE_FEATURE_COUNT] {
        let mut features = [0.0; TYPE_FEATURE_COUNT];
        for arg in &pred.args {
            self.collect_type_features_from_term(arg, &mut features);
        }
        for (_, val) in &pred.named {
            self.collect_type_features_from_term(val, &mut features);
        }
        // Normalize by total arg count to keep values in a reasonable range.
        let total = pred.args.len().max(1) as f32;
        for f in features.iter_mut() {
            *f /= total;
        }
        features
    }

    fn collect_type_features_from_term(&self, term: &Term, features: &mut [f32; TYPE_FEATURE_COUNT]) {
        match term {
            Term::Var(_) => features[0] += 1.0,
            Term::Ent(_) => features[1] += 1.0,
            Term::Lit(l) => {
                features[2] += 1.0;
                match l {
                    Literal::Dec(_, _) => features[5] += 1.0,
                    Literal::Str(_) => features[6] += 1.0,
                    _ => features[7] += 1.0,
                }
            }
            Term::Compound(pred) => {
                features[3] += 1.0;
                for arg in &pred.args {
                    self.collect_type_features_from_term(arg, features);
                }
            }
            Term::List(items) => {
                features[4] += 1.0;
                for item in items {
                    self.collect_type_features_from_term(item, features);
                }
            }
        }
    }

    /// Extract metadata features from a node.
    ///
    /// - \[0]: Confidence (normalized to [0, 1])
    /// - \[1]: Authority (normalized to [0, 1])
    /// - \[2]: Permission level (0=public, 1=internal, 2=confidential, 3=restricted)
    /// - \[3]: Validity type (0=forever, 1=window)
    /// - \[4]: Provenance type (0=verbatim, 1=summary, 2=extracted, 3=derived, 4=asserted)
    /// - \[5]: Has deps (0=no, 1=yes)
    /// - \[6]: Node status (0=active, 1=retracted, 2=pending)
    /// - \[7]: Has note (0=no, 1=yes)
    fn extract_meta_features(&self, node: &Node) -> [f32; META_FEATURE_COUNT] {
        [
            node.confidence.0,
            node.authority.0,
            match node.permissions {
                PermissionTag::PUBLIC => 0.0,
                PermissionTag::INTERNAL => 1.0,
                PermissionTag::CONFIDENTIAL => 2.0,
                PermissionTag::RESTRICTED => 3.0,
                _ => 4.0,
            } / 4.0,
            match node.validity {
                Validity::Forever => 0.0,
                Validity::Window { .. } => 1.0,
            },
            match node.provenance {
                Provenance::Verbatim { .. } => 0.0,
                Provenance::Summary { .. } => 1.0,
                Provenance::Extracted { .. } => 2.0,
                Provenance::Derived { .. } => 3.0,
                Provenance::Asserted { .. } => 4.0,
            } / 4.0,
            if node.deps.is_empty() { 0.0 } else { 1.0 },
            match node.status {
                NodeStatus::Active => 0.0,
                NodeStatus::Retracted => 1.0,
                NodeStatus::Pending => 2.0,
            } / 2.0,
            if node.note.is_some() { 1.0 } else { 0.0 },
        ]
    }

    /// Compute a content hash from the canonical serialization.
    fn content_hash(node: &Node) -> u64 {
        use std::hash::{Hash, Hasher};
        let canonical = serialize::canonical(node);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        canonical.hash(&mut hasher);
        hasher.finish()
    }

    /// Build the full thought vector from features.
    ///
    /// Layout: `[morpheme bag (M) | n-gram TF (N) | type features (8) | meta features (8)]`
    fn build_vector(
        &self,
        morpheme_features: &HashMap<usize, f32>,
        canonical_text: &str,
        type_features: &[f32; TYPE_FEATURE_COUNT],
        meta_features: &[f32; META_FEATURE_COUNT],
    ) -> ThoughtVector {
        let morpheme_count = self.registry.len();
        let mut data = vec![0.0; self.dim];

        // Morpheme bag-of-words features (for head identification).
        for (&idx, &tf) in morpheme_features {
            if idx < morpheme_count {
                data[idx] = tf;
            }
        }

        // N-gram TF-IDF features (for argument order preservation).
        let ngram_features = self.extract_ngram_features(canonical_text);
        for (i, &val) in ngram_features.iter().enumerate() {
            data[morpheme_count + i] = val;
        }

        // Type features.
        for (i, &val) in type_features.iter().enumerate() {
            data[morpheme_count + NGRAM_HASH_SIZE + i] = val;
        }

        // Metadata features.
        for (i, &val) in meta_features.iter().enumerate() {
            data[morpheme_count + NGRAM_HASH_SIZE + TYPE_FEATURE_COUNT + i] = val;
        }

        ThoughtVector { data, dim: self.dim }
    }
}

/// FNV-1a hash function for deterministic n-gram hashing.
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl Encoder for BaselineEncoder {
    fn encode_node(&self, node: &Node) -> Result<EncodeResult, LatentError> {
        let canonical_text = serialize::canonical(node);
        let morpheme_features = self.extract_morpheme_features(&node.predicate);
        let type_features = self.extract_type_features(&node.predicate);
        let meta_features = self.extract_meta_features(node);

        let vector = self.build_vector(&morpheme_features, &canonical_text, &type_features, &meta_features);
        let source_hash = Self::content_hash(node);

        Ok(EncodeResult { vector, source_hash })
    }

    fn encode_sequence(&self, nodes: &[Node]) -> Result<EncodeResult, LatentError> {
        if nodes.is_empty() {
            return Ok(EncodeResult {
                vector: ThoughtVector::zeros(self.dim),
                source_hash: 0,
            });
        }

        let canonical_text = serialize::canonical_all(nodes);

        // Aggregate morpheme, type, and meta features across all nodes.
        let mut combined_morphemes: HashMap<usize, f32> = HashMap::new();
        let mut combined_types = [0.0; TYPE_FEATURE_COUNT];
        let mut combined_meta = [0.0; META_FEATURE_COUNT];

        for node in nodes {
            let mf = self.extract_morpheme_features(&node.predicate);
            for (idx, tf) in mf {
                *combined_morphemes.entry(idx).or_insert(0.0) += tf;
            }

            let tf = self.extract_type_features(&node.predicate);
            for (i, &v) in tf.iter().enumerate() {
                combined_types[i] += v;
            }

            let mf_meta = self.extract_meta_features(node);
            for (i, &v) in mf_meta.iter().enumerate() {
                combined_meta[i] += v;
            }
        }

        // Average the type and meta features.
        let n = nodes.len() as f32;
        for v in combined_types.iter_mut() {
            *v /= n;
        }
        for v in combined_meta.iter_mut() {
            *v /= n;
        }

        let vector = self.build_vector(&combined_morphemes, &canonical_text, &combined_types, &combined_meta);
        let source_hash = {
            use std::hash::{Hash, Hasher};
            let canonical = serialize::canonical_all(nodes);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            canonical.hash(&mut hasher);
            hasher.finish()
        };

        Ok(EncodeResult { vector, source_hash })
    }

    fn encode_text(&self, text: &str) -> Result<EncodeResult, LatentError> {
        let nodes = factum_core::parser::Parser::parse(text)
            .map_err(|e| LatentError::EncodeError(format!("parse error: {}", e)))?;

        if nodes.is_empty() {
            return Ok(EncodeResult {
                vector: ThoughtVector::zeros(self.dim),
                source_hash: 0,
            });
        }

        if nodes.len() == 1 {
            return self.encode_node(&nodes[0]);
        }

        self.encode_sequence(&nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use factum_core::morphemes::MorphemeRegistry;

    fn make_encoder() -> BaselineEncoder {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        BaselineEncoder::new(registry)
    }

    fn make_test_node() -> Node {
        Node::new("n001",
            Predicate::new("shareholder-major")
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
    fn test_encode_node_basic() {
        let enc = make_encoder();
        let node = make_test_node();
        let result = enc.encode_node(&node).unwrap();

        assert_eq!(result.vector.dim, enc.dim());
        assert_eq!(result.vector.data.len(), enc.dim());
        assert_ne!(result.source_hash, 0);
    }

    #[test]
    fn test_encode_produces_nonzero_vector() {
        let enc = make_encoder();
        let node = make_test_node();
        let result = enc.encode_node(&node).unwrap();

        // At least some dimensions should be non-zero.
        let non_zero = result.vector.data.iter().filter(|v| v.abs() > 1e-10).count();
        assert!(non_zero > 0, "vector should have non-zero elements");
    }

    #[test]
    fn test_encode_same_node_same_vector() {
        // Encoding the same node twice should produce identical vectors.
        let enc = make_encoder();
        let node = make_test_node();

        let r1 = enc.encode_node(&node).unwrap();
        let r2 = enc.encode_node(&node).unwrap();

        assert_eq!(r1.vector.data, r2.vector.data);
        assert_eq!(r1.source_hash, r2.source_hash);
    }

    #[test]
    fn test_encode_different_nodes_different_vectors() {
        let enc = make_encoder();

        let node_a = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));
        let node_b = Node::new("n2", Predicate::new("revenue")
            .with_args(vec![Term::ent("ACME-CORP"), Term::lit(Literal::dec_from_str("1000").unwrap())]));

        let r_a = enc.encode_node(&node_a).unwrap();
        let r_b = enc.encode_node(&node_b).unwrap();

        // Vectors should differ.
        assert_ne!(r_a.vector.data, r_b.vector.data);
    }

    #[test]
    fn test_encode_preserves_argument_order() {
        // P0-2 fix: n-gram TF-IDF must distinguish (ceo-of @X @Y)
        // from (ceo-of @Y @X). The old morpheme bag-of-words produced
        // identical vectors for both.
        let enc = make_encoder();

        let node_xy = Node::new("n1", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]));
        let node_yx = Node::new("n2", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("PERSON-X")]));

        let r_xy = enc.encode_node(&node_xy).unwrap();
        let r_yx = enc.encode_node(&node_yx).unwrap();

        // Vectors MUST differ — argument order is now encoded.
        assert_ne!(
            r_xy.vector.data, r_yx.vector.data,
            "n-gram encoder must distinguish argument order: (ceo-of @X @Y) != (ceo-of @Y @X)"
        );
    }

    #[test]
    fn test_encode_sequence() {
        let enc = make_encoder();

        let nodes = vec![
            Node::new("n1", Predicate::new("located-in")
                .with_args(vec![Term::ent("X"), Term::ent("Y")])),
            Node::new("n2", Predicate::new("ceo-of")
                .with_args(vec![Term::ent("P"), Term::ent("X")])),
        ];

        let result = enc.encode_sequence(&nodes).unwrap();
        assert_eq!(result.vector.dim, enc.dim());
        assert_ne!(result.source_hash, 0);
    }

    #[test]
    fn test_encode_empty_sequence() {
        let enc = make_encoder();
        let result = enc.encode_sequence(&[]).unwrap();
        assert_eq!(result.vector.dim, enc.dim());
        assert_eq!(result.source_hash, 0);
    }

    #[test]
    fn test_encode_text() {
        let enc = make_encoder();
        let text = r#"(node n001 :pred (located-in @ACME-CORP @CUPERTINO) :valid forever :src (asserted "test") :conf 0.85 :auth 0.9 :perm confidential)"#;

        let result = enc.encode_text(text).unwrap();
        assert_eq!(result.vector.dim, enc.dim());
        assert_ne!(result.source_hash, 0);
    }

    #[test]
    fn test_encode_text_invalid() {
        let enc = make_encoder();
        // Unclosed parenthesis is definitely invalid.
        let result = enc.encode_text("(node :pred (");
        assert!(result.is_err());
    }

    #[test]
    fn test_thought_vector_l2_norm() {
        let v = ThoughtVector::from_vec(vec![3.0, 4.0]);
        let norm = v.l2_norm();
        assert!((norm - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_thought_vector_normalize() {
        let v = ThoughtVector::from_vec(vec![3.0, 4.0]);
        let n = v.normalize();
        assert!((n.l2_norm() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_thought_vector_cosine_similarity() {
        let a = ThoughtVector::from_vec(vec![1.0, 0.0]);
        let b = ThoughtVector::from_vec(vec![1.0, 0.0]);
        assert!((a.cosine_similarity(&b) - 1.0).abs() < 0.001);

        let c = ThoughtVector::from_vec(vec![0.0, 1.0]);
        assert!((a.cosine_similarity(&c).abs()) < 0.001);
    }

    #[test]
    fn test_thought_vector_dim_mismatch() {
        let a = ThoughtVector::from_vec(vec![1.0, 0.0]);
        let b = ThoughtVector::from_vec(vec![1.0, 0.0, 0.0]);
        assert_eq!(a.cosine_similarity(&b), 0.0);
    }

    #[test]
    fn test_metadata_features_encoded() {
        // Nodes with different confidence should produce different vectors.
        let enc = make_encoder();

        let pred = Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]);
        let a = Node::new("n1", pred.clone()).with_confidence(Confidence(0.5));
        let b = Node::new("n2", pred).with_confidence(Confidence(1.0));

        let r_a = enc.encode_node(&a).unwrap();
        let r_b = enc.encode_node(&b).unwrap();

        // The meta feature dimension for confidence should differ.
        let meta_offset = enc.meta_offset();
        assert_ne!(r_a.vector.data[meta_offset], r_b.vector.data[meta_offset]);
    }
}
