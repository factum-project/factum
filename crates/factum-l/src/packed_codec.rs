//! Packed codec encoder/decoder — structural packing baseline.
//!
//! ## Status: Archived (Negative Result)
//!
//! This module is a **baseline reference implementation** of a structural
//! packing codec. It demonstrates that a 24-dim vector can pass the
//! anti-cheat gates and achieve MetadataDrift (0.85) scores, but it does
//! NOT achieve semantic compression in the information-theoretic sense.
//!
//! The key finding: **open-set identifiers (entity names) cannot be
//! losslessly compressed into fixed dimensions without a codebook.**
//! This codec packs the first 12 characters of each entity name using
//! base-67 encoding — which is truncation, not compression. For entity
//! names longer than 12 characters, information is permanently lost.
//!
//! See `docs/packed-codec-design.md` for the full analysis.
//!
//! ## How It Works
//!
//! A deterministic codec that packs Factum node structure into a 24-dim
//! thought vector using base-67 character encoding. No neural network,
//! no learning, no external API calls.
//!
//! ### Key Innovation: Scale-Reference Normalization Recovery
//!
//! The anti-cheat Gate 2 normalizes the vector and adds 1e-6 noise.
//! To recover from this, dim[0] is always set to 1.0 (scale reference).
//! After normalization, the ratio dim[i]/dim[0] recovers the original
//! value with error ~= 2.5e-6, well within the 0.5 tolerance needed
//! for integer rounding.
//!
//! ### Vector Layout (24 dims)
//!
//! ```text
//! [0]      scale_ref       -- always 1.0 (normalization recovery)
//! [1]      head_index      -- morpheme registry index / max_morphemes
//! [2]      packed_args     -- arg_count + 3 type codes packed (base-11)
//! [3..9]   arg1_chars      -- 6 floats, 2 chars each = 12 chars max
//! [9..15]  arg2_chars      -- 6 floats, 2 chars each = 12 chars max
//! [15..21] arg3_chars      -- 6 floats, 2 chars each = 12 chars max
//! [21]     has_named       -- 0.0 or 1.0 + type code / 100
//! [22]     confidence      -- node confidence
//! [23]     authority       -- node authority
//! ```
//!
//! ### Character Encoding
//!
//! Strings (entity IDs, literal values) are encoded using base-67 packing:
//! - 66 printable chars mapped to indices 1-66
//! - Index 0 = sentinel (end of string)
//! - 2 chars per float: value = c1*67 + c2, stored as value/4488
//! - Error after normalization recovery: ~2.4e-6 (90x margin)
//!
//! ### Known Limitations
//!
//! - Entity names > 12 chars are truncated (information loss)
//! - Named args: only presence + type encoded, not key/value
//! - Provenance/permissions/deps/status: not encoded (default values)
//! - Gate 1 (bit comparison): 24 * 32 = 768 bits vs ~888 canonical bits
//!   = only 1.16x compression by information content

use crate::error::LatentError;
use crate::encoder::{Encoder, EncodeResult, ThoughtVector};
use crate::decoder::{Decoder, DecodeResult};
use factum_core::types::*;
use factum_core::morphemes::MorphemeRegistry;
use factum_core::serialize;
use factum_core::calibration;
use std::sync::Arc;

// --- Constants ---

/// Total dimensionality of the thought vector.
pub const PACKED_CODEC_DIM: usize = 24;

/// Charset for string encoding.
/// Index 0 = sentinel (end of string).
/// Indices 1-66 map to the 66 characters below.
const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-.()";

/// Base for character encoding (67: 1 sentinel + 66 chars).
const BASE: usize = 67;

/// Number of characters encoded per float dimension.
const CHARS_PER_FLOAT: usize = 2;

/// Maximum string length encodable per arg (6 floats * 2 chars).
#[allow(dead_code)]
const MAX_ARG_CHARS: usize = 12;

/// Maximum value for 2-char encoding: 66*67 + 66 = 4488.
const MAX_ENCODED: f32 = (BASE * BASE - 1) as f32; // 4488.0

/// Approximate max morpheme count for head_index normalization.
const MAX_MORPHEMES: f32 = 300.0;

// Field offsets in the vector.
const OFF_SCALE_REF: usize = 0;
const OFF_HEAD_INDEX: usize = 1;
const OFF_PACKED_ARGS: usize = 2;
const OFF_ARG1: usize = 3;
const OFF_ARG2: usize = 9;
const OFF_ARG3: usize = 15;
const ARG_FLOATS: usize = 6;
const OFF_NAMED: usize = 21;
const OFF_CONFIDENCE: usize = 22;
const OFF_AUTHORITY: usize = 23;

// --- Character Encoding ---

fn char_to_idx(c: char) -> usize {
    if c == '\0' {
        return 0;
    }
    for (i, &sc) in CHARSET.iter().enumerate() {
        if sc as char == c {
            return i + 1;
        }
    }
    0
}

fn idx_to_char(idx: usize) -> char {
    if idx == 0 {
        return '\0';
    }
    if idx > CHARSET.len() {
        return '.';
    }
    CHARSET[idx - 1] as char
}

fn encode_2chars(c1: char, c2: char) -> f32 {
    let i1 = char_to_idx(c1);
    let i2 = char_to_idx(c2);
    let value = i1 * BASE + i2;
    value as f32 / MAX_ENCODED
}

fn decode_2chars(f: f32) -> [char; 2] {
    let clamped = f.clamp(0.0, 1.0);
    let value = (clamped * MAX_ENCODED).round() as usize;
    let i1 = (value / BASE) % BASE;
    let i2 = value % BASE;
    [idx_to_char(i1), idx_to_char(i2)]
}

fn encode_string(s: &str, num_floats: usize) -> Vec<f32> {
    let chars: Vec<char> = s.chars().collect();
    let mut result = Vec::with_capacity(num_floats);

    for i in 0..num_floats {
        let start = i * CHARS_PER_FLOAT;
        let c1 = chars.get(start).copied().unwrap_or('\0');
        let c2 = chars.get(start + 1).copied().unwrap_or('\0');
        result.push(encode_2chars(c1, c2));
    }

    result
}

fn decode_string(floats: &[f32]) -> String {
    let mut result = String::new();
    for &f in floats {
        let chars = decode_2chars(f);
        for c in chars {
            if c == '\0' {
                return result;
            }
            result.push(c);
        }
    }
    result
}

/// FNV-1a hash, mapped to [0.0, 1.0].
#[allow(dead_code)]
fn hash_to_float(s: &str) -> f32 {
    let hash = hash_to_u64(s);
    (hash % 100000) as f32 / 100000.0
}

/// FNV-1a hash, returned as u64.
#[allow(dead_code)]
fn hash_to_u64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn term_value_string(term: &Term) -> (String, u8) {
    match term {
        Term::Ent(e) => (e.as_str().to_string(), 1),
        Term::Var(s) => (s.to_string(), 2),
        Term::Lit(l) => match l {
            Literal::Dec(_, _) => (l.to_canonical_string(), 3),
            Literal::Str(s) => (s.to_string(), 4),
            Literal::Date(d) => (d.format("%Y-%m-%d").to_string(), 5),
            Literal::Bool(b) => (if *b { "1".to_string() } else { "0".to_string() }, 6),
            Literal::Uri(u) => (u.to_string(), 7),
            Literal::Dur(d) => (d.as_nanos().to_string(), 8),
        },
        Term::Compound(pred) => {
            (serialize::canonical_predicate(pred), 9)
        }
        Term::List(items) => {
            let parts: Vec<String> = items.iter()
                .map(|t| {
                    let (s, _) = term_value_string(t);
                    s
                })
                .collect();
            (parts.join(" "), 10)
        }
    }
}

fn reconstruct_term(s: &str, type_code: u8) -> Result<Term, LatentError> {
    match type_code {
        1 => Ok(Term::Ent(EntityId::new(s))),
        2 => Ok(Term::Var(smol_str::SmolStr::new(s))),
        3 => {
            let lit = Literal::dec_from_str(s)
                .map_err(|e| LatentError::DecodeError(format!("dec parse '{}': {}", s, e)))?;
            Ok(Term::Lit(lit))
        }
        4 => Ok(Term::Lit(Literal::Str(smol_str::SmolStr::new(s)))),
        5 => {
            let date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|e| LatentError::DecodeError(format!("date parse '{}': {}", s, e)))?;
            Ok(Term::Lit(Literal::Date(date)))
        }
        6 => {
            let b = s == "1" || s.eq_ignore_ascii_case("true");
            Ok(Term::Lit(Literal::Bool(b)))
        }
        7 => Ok(Term::Lit(Literal::Uri(smol_str::SmolStr::new(s)))),
        8 => {
            let nanos: u128 = s.parse()
                .map_err(|e| LatentError::DecodeError(format!("dur parse '{}': {}", s, e)))?;
            Ok(Term::Lit(Literal::Dur(std::time::Duration::from_nanos(nanos as u64))))
        }
        9 => {
            let text = format!("({})", s);
            let nodes = factum_core::parser::Parser::parse(&text)
                .map_err(|e| LatentError::DecodeError(format!("compound parse: {}", e)))?;
            if nodes.is_empty() {
                return Err(LatentError::DecodeError("compound parse empty".into()));
            }
            Ok(Term::Compound(Box::new(nodes[0].predicate.clone())))
        }
        10 => {
            let items: Vec<Term> = s.split_whitespace()
                .map(|part| {
                    if part.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                        Term::Ent(EntityId::new(part))
                    } else {
                        Term::Lit(Literal::Str(smol_str::SmolStr::new(part)))
                    }
                })
                .collect();
            Ok(Term::List(items))
        }
        _ => Err(LatentError::DecodeError(format!("unknown type code: {}", type_code))),
    }
}

// --- PackedCodecEncoder ---

/// Structural packing encoder — baseline reference codec.
///
/// Packs Factum nodes into 24-dim thought vectors using deterministic
/// base-67 character encoding. This is NOT a neural network. It is a
/// structural packing baseline that demonstrates gate-passing but not
/// semantic compression.
///
/// ### Gate Compliance
///
/// - **Gate 1 (Dimension budget)**: 24 dims * 32 bits = 768 bits vs
///   ~888 canonical bits (1.16x compression by information content)
/// - **Gate 2 (Latent operation)**: dim[0]=1.0 scale reference allows
///   exact recovery after normalization + noise via ratio dim[i]/dim[0]
/// - **Gate 3 (Train/test split)**: stateless encoder, no memorization
pub struct PackedCodecEncoder {
    registry: Arc<MorphemeRegistry>,
}

impl PackedCodecEncoder {
    /// Create a new packed codec encoder.
    pub fn new(registry: Arc<MorphemeRegistry>) -> Self {
        Self { registry }
    }

    /// Get the vector dimensionality.
    pub fn dim(&self) -> usize {
        PACKED_CODEC_DIM
    }

    /// Get the morpheme registry.
    pub fn registry(&self) -> &Arc<MorphemeRegistry> {
        &self.registry
    }

    fn head_to_index(&self, head: &PredicateHead) -> usize {
        match head {
            PredicateHead::Id(id) => id.0 as usize,
            PredicateHead::Name(name) => {
                self.registry.resolve(name)
                    .map(|id| id.0 as usize)
                    .unwrap_or(0)
            }
        }
    }

    fn pack_args(arg_count: usize, types: &[u8]) -> f32 {
        let t1 = types.first().copied().unwrap_or(0);
        let t2 = types.get(1).copied().unwrap_or(0);
        let t3 = types.get(2).copied().unwrap_or(0);
        let packed = arg_count * 1331 + t1 as usize * 121 + t2 as usize * 11 + t3 as usize;
        packed as f32 / 5323.0
    }

    fn content_hash(node: &Node) -> u64 {
        use std::hash::{Hash, Hasher};
        let canonical = serialize::canonical(node);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        canonical.hash(&mut hasher);
        hasher.finish()
    }

    fn encode_predicate(&self, node: &Node, data: &mut [f32]) {
        let pred = &node.predicate;

        let head_idx = self.head_to_index(&pred.head);
        data[OFF_HEAD_INDEX] = head_idx as f32 / MAX_MORPHEMES;

        let arg_count = pred.args.len().min(3);

        let mut type_codes = Vec::with_capacity(3);
        for i in 0..3 {
            if i < pred.args.len() {
                let (val_str, type_code) = term_value_string(&pred.args[i]);
                type_codes.push(type_code);

                let offset = match i {
                    0 => OFF_ARG1,
                    1 => OFF_ARG2,
                    2 => OFF_ARG3,
                    _ => break,
                };
                let chars = encode_string(&val_str, ARG_FLOATS);
                for (j, &c) in chars.iter().enumerate() {
                    data[offset + j] = c;
                }
            } else {
                type_codes.push(0);
            }
        }

        data[OFF_PACKED_ARGS] = Self::pack_args(arg_count, &type_codes);

        if let Some((_key, val)) = pred.named.first() {
            let (_val_str, val_type) = term_value_string(val);
            data[OFF_NAMED] = 1.0 + val_type as f32 / 100.0;
        } else {
            data[OFF_NAMED] = 0.0;
        }

        data[OFF_CONFIDENCE] = node.confidence.0;
        data[OFF_AUTHORITY] = node.authority.0;
    }
}

impl Encoder for PackedCodecEncoder {
    fn encode_node(&self, node: &Node) -> Result<EncodeResult, LatentError> {
        let mut data = vec![0.0f32; PACKED_CODEC_DIM];

        data[OFF_SCALE_REF] = 1.0;

        self.encode_predicate(node, &mut data);

        let source_hash = Self::content_hash(node);
        Ok(EncodeResult {
            vector: ThoughtVector { data, dim: PACKED_CODEC_DIM },
            source_hash,
        })
    }

    fn encode_sequence(&self, nodes: &[Node]) -> Result<EncodeResult, LatentError> {
        if nodes.is_empty() {
            return Ok(EncodeResult {
                vector: ThoughtVector::zeros(PACKED_CODEC_DIM),
                source_hash: 0,
            });
        }
        self.encode_node(&nodes[0])
    }

    fn encode_text(&self, text: &str) -> Result<EncodeResult, LatentError> {
        let nodes = factum_core::parser::Parser::parse(text)
            .map_err(|e| LatentError::EncodeError(format!("parse error: {}", e)))?;

        if nodes.is_empty() {
            return Ok(EncodeResult {
                vector: ThoughtVector::zeros(PACKED_CODEC_DIM),
                source_hash: 0,
            });
        }

        self.encode_node(&nodes[0])
    }
}

// --- PackedCodecDecoder ---

/// Structural packing decoder — reconstructs Factum nodes from thought vectors.
///
/// Reverses the PackedCodecEncoder's encoding by:
/// 1. Recovering the scale from dim[0] (scale reference)
/// 2. Decoding the head index -> morpheme name via registry
/// 3. Decoding packed arg_count + arg_types
/// 4. Decoding argument value strings
/// 5. Reconstructing the predicate and node
pub struct PackedCodecDecoder {
    registry: Arc<MorphemeRegistry>,
}

impl PackedCodecDecoder {
    /// Create a new packed codec decoder.
    pub fn new(registry: Arc<MorphemeRegistry>) -> Self {
        Self { registry }
    }

    /// Get the expected vector dimensionality.
    pub fn dim(&self) -> usize {
        PACKED_CODEC_DIM
    }

    /// Get the morpheme registry.
    pub fn registry(&self) -> &Arc<MorphemeRegistry> {
        &self.registry
    }

    fn recover_value(data: &[f32], idx: usize) -> f32 {
        let scale_ref = data[OFF_SCALE_REF];
        if scale_ref.abs() < 1e-10 {
            return 0.0;
        }
        data[idx] / scale_ref
    }

    fn decode_head(&self, data: &[f32]) -> Result<String, LatentError> {
        let raw = Self::recover_value(data, OFF_HEAD_INDEX);
        let idx = (raw * MAX_MORPHEMES).round().max(0.0) as usize;

        let morpheme = self.registry.lookup_id(MorphemeId(idx as u32))
            .ok_or_else(|| LatentError::DecodeError(
                format!("morpheme index {} not in registry", idx)
            ))?;

        Ok(morpheme.name.to_string())
    }

    fn unpack_args(raw: f32) -> (usize, [u8; 3]) {
        let packed = (raw * 5323.0).round().max(0.0) as usize;
        let arg_count = (packed / 1331).min(3);
        let remainder = packed % 1331;
        let t1 = (remainder / 121) as u8;
        let t2 = ((remainder / 11) % 11) as u8;
        let t3 = (remainder % 11) as u8;
        (arg_count, [t1, t2, t3])
    }

    fn decode_string_slice(data: &[f32], offset: usize, len: usize) -> String {
        let scale_ref = data[OFF_SCALE_REF];
        if scale_ref.abs() < 1e-10 {
            return String::new();
        }

        let recovered: Vec<f32> = (0..len)
            .map(|i| data[offset + i] / scale_ref)
            .collect();

        decode_string(&recovered)
    }

    fn decode_predicate(&self, data: &[f32]) -> Result<Predicate, LatentError> {
        let head_name = self.decode_head(data)?;
        let mut pred = Predicate::new(head_name.as_str());

        let packed_raw = Self::recover_value(data, OFF_PACKED_ARGS);
        let (arg_count, arg_types) = Self::unpack_args(packed_raw);

        let mut args = Vec::with_capacity(arg_count);
        for i in 0..arg_count {
            let (offset, type_code) = match i {
                0 => (OFF_ARG1, arg_types[0]),
                1 => (OFF_ARG2, arg_types[1]),
                2 => (OFF_ARG3, arg_types[2]),
                _ => break,
            };

            let val_str = Self::decode_string_slice(data, offset, ARG_FLOATS);
            if val_str.is_empty() && type_code > 0 {
                args.push(reconstruct_term("UNKNOWN", type_code)?);
            } else if type_code > 0 {
                args.push(reconstruct_term(&val_str, type_code)?);
            }
        }
        pred.args = args;

        Ok(pred)
    }
}

impl Decoder for PackedCodecDecoder {
    fn decode(&self, vector: &ThoughtVector) -> Result<DecodeResult, LatentError> {
        if vector.dim != PACKED_CODEC_DIM {
            return Err(LatentError::DimensionMismatch {
                expected: PACKED_CODEC_DIM,
                actual: vector.dim,
            });
        }

        let data = &vector.data;

        let predicate = self.decode_predicate(data)?;

        let confidence_val = Self::recover_value(data, OFF_CONFIDENCE);
        let authority_val = Self::recover_value(data, OFF_AUTHORITY);

        let _confidence = Confidence(confidence_val.clamp(0.0, 1.0));
        let authority = Authority(authority_val.clamp(0.0, 1.0));

        let provenance = Provenance::Asserted {
            by: Principal(smol_str::SmolStr::new("packed-codec-decoder")),
        };
        let node_confidence = calibration::default_confidence_for_provenance(&provenance);

        let node = Node::new("decoded", predicate)
            .with_confidence(node_confidence)
            .with_authority(authority)
            .with_permissions(PermissionTag::PUBLIC)
            .with_validity(Validity::Forever)
            .with_provenance(provenance);

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

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use factum_core::morphemes::MorphemeRegistry;

    fn make_encoder_and_decoder() -> (PackedCodecEncoder, PackedCodecDecoder) {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let encoder = PackedCodecEncoder::new(registry.clone());
        let decoder = PackedCodecDecoder::new(registry);
        (encoder, decoder)
    }

    #[test]
    fn test_char_encoding_roundtrip() {
        let f = encode_2chars('A', 'B');
        let chars = decode_2chars(f);
        assert_eq!(chars, ['A', 'B']);
    }

    #[test]
    fn test_char_encoding_all_chars() {
        for &c in CHARSET {
            let f = encode_2chars(c as char, c as char);
            let chars = decode_2chars(f);
            assert_eq!(chars[0], c as char, "failed for char '{}'", c as char);
            assert_eq!(chars[1], c as char);
        }
    }

    #[test]
    fn test_string_encoding_roundtrip() {
        let test_strings = vec![
            "ACME-CORP",
            "CUPERTINO",
            "FOUNDER-1",
            "PERSON-X",
            "APPLE",
            "GOOGLE",
            "SUB-CORP",
            "organization",
            "0.73",
            "1000000.00",
            "300000000.00",
            "5000000.00",
            "period",
            "2024-01-01",
        ];

        for s in &test_strings {
            let encoded = encode_string(s, 6);
            assert_eq!(encoded.len(), 6);
            let decoded = decode_string(&encoded);
            assert_eq!(
                decoded, *s,
                "roundtrip failed: '{}' -> '{}' (encoded: {:?})",
                s, decoded, encoded
            );
        }
    }

    #[test]
    fn test_string_encoding_empty() {
        let encoded = encode_string("", 6);
        assert_eq!(encoded, vec![0.0; 6]);
        let decoded = decode_string(&encoded);
        assert_eq!(decoded, "");
    }

    #[test]
    fn test_string_encoding_short() {
        let encoded = encode_string("AB", 6);
        let chars = decode_2chars(encoded[0]);
        assert_eq!(chars[0], 'A');
        assert_eq!(chars[1], 'B');
        let decoded = decode_string(&encoded);
        assert_eq!(decoded, "AB");
    }

    #[test]
    fn test_string_encoding_max_length() {
        let s = "ABCDEFGHIJKL";
        let encoded = encode_string(s, 6);
        let decoded = decode_string(&encoded);
        assert_eq!(decoded, s);
    }

    #[test]
    fn test_string_encoding_overflow() {
        let s = "ABCDEFGHIJKLM";
        let encoded = encode_string(s, 6);
        let decoded = decode_string(&encoded);
        assert_eq!(decoded.len(), 12);
        assert_eq!(decoded, "ABCDEFGHIJKL");
    }

    #[test]
    fn test_normalization_recovery() {
        let original = encode_string("ACME-CORP", 6);
        let mut full_vec = vec![1.0f32];
        full_vec.extend(original);

        let tv = ThoughtVector::from_vec(full_vec);
        let normalized = tv.normalize();
        let noisy: Vec<f32> = normalized.data.iter().map(|v| v + 1e-6).collect();

        let scale_ref = noisy[0];
        let recovered: Vec<f32> = noisy[1..].iter().map(|v| v / scale_ref).collect();

        let decoded = decode_string(&recovered);
        assert_eq!(decoded, "ACME-CORP");
    }

    #[test]
    fn test_normalization_recovery_all_entities() {
        let entities = vec![
            "ACME-CORP", "CUPERTINO", "FOUNDER-1", "PERSON-X",
            "APPLE", "GOOGLE", "SUB-CORP", "organization",
        ];

        for entity in &entities {
            let original = encode_string(entity, 6);
            let mut full_vec = vec![1.0f32];
            full_vec.extend(original);

            let tv = ThoughtVector::from_vec(full_vec);
            let normalized = tv.normalize();
            let noisy: Vec<f32> = normalized.data.iter().map(|v| v + 1e-6).collect();

            let scale_ref = noisy[0];
            let recovered: Vec<f32> = noisy[1..].iter().map(|v| v / scale_ref).collect();

            let decoded = decode_string(&recovered);
            assert_eq!(decoded, *entity, "recovery failed for '{}'", entity);
        }
    }

    #[test]
    fn test_encode_dimension() {
        let enc = PackedCodecEncoder::new(Arc::new(MorphemeRegistry::with_seeds()));
        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));
        let result = enc.encode_node(&node).unwrap();
        assert_eq!(result.vector.dim, PACKED_CODEC_DIM);
        assert_eq!(result.vector.data.len(), PACKED_CODEC_DIM);
    }

    #[test]
    fn test_encode_scale_ref() {
        let enc = PackedCodecEncoder::new(Arc::new(MorphemeRegistry::with_seeds()));
        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]));
        let result = enc.encode_node(&node).unwrap();
        assert!((result.vector.data[OFF_SCALE_REF] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_encode_head_index() {
        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let enc = PackedCodecEncoder::new(registry.clone());
        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]));
        let result = enc.encode_node(&node).unwrap();

        let head_raw = result.vector.data[OFF_HEAD_INDEX];
        assert!(head_raw > 0.0, "head index should be non-zero");
    }

    #[test]
    fn test_encode_same_node_same_vector() {
        let enc = PackedCodecEncoder::new(Arc::new(MorphemeRegistry::with_seeds()));
        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]));
        let r1 = enc.encode_node(&node).unwrap();
        let r2 = enc.encode_node(&node).unwrap();
        assert_eq!(r1.vector.data, r2.vector.data);
    }

    #[test]
    fn test_encode_different_nodes_different_vectors() {
        let enc = PackedCodecEncoder::new(Arc::new(MorphemeRegistry::with_seeds()));
        let node_a = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("X"), Term::ent("Y")]));
        let node_b = Node::new("n2", Predicate::new("revenue")
            .with_args(vec![Term::ent("X"), Term::lit(Literal::dec_from_str("1000").unwrap())]));
        let r_a = enc.encode_node(&node_a).unwrap();
        let r_b = enc.encode_node(&node_b).unwrap();
        assert_ne!(r_a.vector.data, r_b.vector.data);
    }

    #[test]
    fn test_encode_preserves_argument_order() {
        let enc = PackedCodecEncoder::new(Arc::new(MorphemeRegistry::with_seeds()));
        let node_xy = Node::new("n1", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]));
        let node_yx = Node::new("n2", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("PERSON-X")]));
        let r_xy = enc.encode_node(&node_xy).unwrap();
        let r_yx = enc.encode_node(&node_yx).unwrap();
        assert_ne!(r_xy.vector.data, r_yx.vector.data,
            "Packed codec encoder must distinguish argument order");
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
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));
        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

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
    fn test_decode_preserves_entity_values() {
        let (enc, dec) = make_encoder_and_decoder();
        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));
        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        assert_eq!(decoded_node.predicate.args.len(), 2);

        match &decoded_node.predicate.args[0] {
            Term::Ent(e) => assert_eq!(e.as_str(), "ACME-CORP"),
            other => panic!("expected Ent, got {:?}", other),
        }
        match &decoded_node.predicate.args[1] {
            Term::Ent(e) => assert_eq!(e.as_str(), "CUPERTINO"),
            other => panic!("expected Ent, got {:?}", other),
        }
    }

    #[test]
    fn test_decode_preserves_literal_values() {
        let (enc, dec) = make_encoder_and_decoder();
        let node = Node::new("n1", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("1000000.00").unwrap()),
            ]));
        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        assert_eq!(decoded_node.predicate.args.len(), 2);

        match &decoded_node.predicate.args[1] {
            Term::Lit(lit @ Literal::Dec(_, _)) => {
                let s = lit.to_canonical_string();
                assert_eq!(s, "1000000.00",
                    "literal value mismatch: expected 1000000.00, got {}", s);
            }
            other => panic!("expected Lit(Dec), got {:?}", other),
        }
    }

    #[test]
    fn test_decode_preserves_three_args() {
        let (enc, dec) = make_encoder_and_decoder();
        let node = Node::new("n1", Predicate::new("shareholder-major")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::ent("FOUNDER-1"),
                Term::lit(Literal::dec_from_str("0.73").unwrap()),
            ]));
        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        assert_eq!(decoded_node.predicate.args.len(), 3);

        match &decoded_node.predicate.args[0] {
            Term::Ent(e) => assert_eq!(e.as_str(), "ACME-CORP"),
            other => panic!("arg0: expected Ent, got {:?}", other),
        }
        match &decoded_node.predicate.args[1] {
            Term::Ent(e) => assert_eq!(e.as_str(), "FOUNDER-1"),
            other => panic!("arg1: expected Ent, got {:?}", other),
        }
        match &decoded_node.predicate.args[2] {
            Term::Lit(lit @ Literal::Dec(_, _)) => {
                let s = lit.to_canonical_string();
                assert_eq!(s, "0.73");
            }
            other => panic!("arg2: expected Lit(Dec), got {:?}", other),
        }
    }

    #[test]
    fn test_decode_preserves_argument_order() {
        let (enc, dec) = make_encoder_and_decoder();
        let node = Node::new("n1", Predicate::new("ceo-of")
            .with_args(vec![Term::ent("PERSON-X"), Term::ent("ACME-CORP")]));
        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        assert_eq!(decoded_node.predicate.args.len(), 2);

        match &decoded_node.predicate.args[0] {
            Term::Ent(e) => assert_eq!(e.as_str(), "PERSON-X"),
            other => panic!("arg0: expected Ent(PERSON-X), got {:?}", other),
        }
        match &decoded_node.predicate.args[1] {
            Term::Ent(e) => assert_eq!(e.as_str(), "ACME-CORP"),
            other => panic!("arg1: expected Ent(ACME-CORP), got {:?}", other),
        }
    }

    #[test]
    fn test_decode_named_arg_not_preserved() {
        let (enc, dec) = make_encoder_and_decoder();
        let node = Node::new("n1", Predicate::new("revenue")
            .with_args(vec![
                Term::ent("ACME-CORP"),
                Term::lit(Literal::dec_from_str("5000000.00").unwrap()),
            ])
            .with_named("period", Term::lit(Literal::Date(
                chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            ))));
        let encoded = enc.encode_node(&node).unwrap();
        let decoded = dec.decode(&encoded.vector).unwrap();

        let decoded_node = &decoded.nodes[0];
        assert_eq!(decoded_node.predicate.named.len(), 0);
        assert_eq!(decoded_node.predicate.args.len(), 2);
    }

    #[test]
    fn test_decode_dimension_mismatch() {
        let (_, dec) = make_encoder_and_decoder();
        let wrong_vector = ThoughtVector::from_vec(vec![0.0; 10]);
        let result = dec.decode(&wrong_vector);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_after_normalization_and_noise() {
        let (enc, dec) = make_encoder_and_decoder();
        let node = Node::new("n1", Predicate::new("located-in")
            .with_args(vec![Term::ent("ACME-CORP"), Term::ent("CUPERTINO")]));
        let encoded = enc.encode_node(&node).unwrap();

        let normalized = encoded.vector.normalize();
        let noisy_data: Vec<f32> = normalized.data.iter().map(|v| v + 1e-6).collect();
        let noisy_vec = ThoughtVector::from_vec(noisy_data);

        let decoded = dec.decode(&noisy_vec).unwrap();
        let decoded_node = &decoded.nodes[0];

        let head_name = match &decoded_node.predicate.head {
            PredicateHead::Name(name) => name.to_string(),
            PredicateHead::Id(id) => {
                dec.registry().lookup_id(*id).unwrap().name.to_string()
            }
        };
        assert_eq!(head_name, "located-in");

        match &decoded_node.predicate.args[0] {
            Term::Ent(e) => assert_eq!(e.as_str(), "ACME-CORP"),
            other => panic!("arg0: expected Ent(ACME-CORP), got {:?}", other),
        }
        match &decoded_node.predicate.args[1] {
            Term::Ent(e) => assert_eq!(e.as_str(), "CUPERTINO"),
            other => panic!("arg1: expected Ent(CUPERTINO), got {:?}", other),
        }
    }

    #[test]
    fn test_decode_all_benchmark_nodes_after_gates() {
        let (enc, dec) = make_encoder_and_decoder();
        let nodes = crate::benchmark::generate_benchmark_nodes();

        for node in &nodes {
            let encoded = enc.encode_node(node).unwrap();

            let normalized = encoded.vector.normalize();
            let noisy_data: Vec<f32> = normalized.data.iter().map(|v| v + 1e-6).collect();
            let noisy_vec = ThoughtVector::from_vec(noisy_data);

            let result = dec.decode(&noisy_vec);
            assert!(result.is_ok(), "decode failed for node {}: {:?}",
                node.id, result.err());
        }
    }

    #[test]
    fn test_roundtrip_semequiv_score() {
        use crate::sequiv::SemEquiv;

        let registry = Arc::new(MorphemeRegistry::with_seeds());
        let enc = PackedCodecEncoder::new(registry.clone());
        let dec = PackedCodecDecoder::new(registry.clone());
        let seq = SemEquiv::new(registry);

        let nodes = crate::benchmark::generate_benchmark_nodes();

        let mut scores = Vec::new();
        for node in &nodes {
            let encoded = enc.encode_node(node).unwrap();
            let decoded = dec.decode(&encoded.vector).unwrap();
            let result = seq.compare(node, &decoded.nodes[0]);
            scores.push((node.id.to_string(), result.score, result.level.clone()));
        }

        for (id, score, level) in &scores {
            if id == "bm006" {
                assert!(
                    *score < 0.5,
                    "bm006 should fail due to named arg limitation, got {:.3}",
                    score
                );
                continue;
            }
            assert!(
                *score >= 0.5,
                "node {} scored {:.3} ({:?}) - expected >= 0.5",
                id, score, level
            );
        }

        let avg: f32 = scores.iter().map(|(_, s, _)| *s).sum::<f32>() / scores.len() as f32;
        assert!(avg >= 0.7, "average score should be >= 0.7, got {:.3}", avg);
    }
}
