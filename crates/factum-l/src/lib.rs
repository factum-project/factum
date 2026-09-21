//! # factum-l
//!
//! **Latent space projection for Factum — encoder/decoder and semantic equivalence.**
//!
//! ## Status: Archived (Negative Result)
//!
//! factum-l is a research experiment that has been measured and concluded
//! with a negative result. The hypothesis — that structured knowledge can
//! be compressed into low-dimensional vectors and reconstructed with high
//! fidelity — was tested and found to be blocked by an information-theoretic
//! barrier: open-set identifiers (entity names) cannot be losslessly
//! compressed into fixed dimensions without a codebook.
//!
//! ## Deliverables
//!
//! - [`SemEquiv`]: Structured semantic equivalence comparison (production-ready,
//!   independently useful for deduplication, conflict detection, upsert verification)
//! - [`benchmark`]`: Anti-cheat gates + measurement methodology
//! - [`packed_codec`]: Structural packing baseline (demonstrates gate-passing
//!   but not semantic compression)
//!
//! ## Modules
//! - [`encoder`]: Encoder trait + BaselineEncoder (morpheme bag + n-gram TF-IDF)
//! - [`decoder`]: Decoder trait + BaselineDecoder (nearest-neighbor + template)
//! - [`packed_codec`]: PackedCodecEncoder + PackedCodecDecoder (24-dim structural
//!   packing baseline — passes gates, does not achieve semantic compression)
//! - [`sequiv`]: SemEquiv — structured semantic equivalence comparison
//!   (**independently useful** for production)
//! - [`benchmark`]: Semantic round-trip benchmark with anti-cheat gates
//! - [`error`]: LatentError error type
//!
//! ## What Has Independent Value
//!
//! [`SemEquiv`] is the only component with independent production value.
//! The encoder, decoder, and benchmark exist to measure information loss
//! during latent space projection. The measurement is complete; the result
//! is negative.
//!
//! ## Encoder Comparison
//!
//! | Encoder | Dims | Gate 1 (bits) | Gated Avg Score |
//! |---------|------|---------------|-----------------|
//! | BaselineEncoder | 502 | FAIL | 0.000 |
//! | PackedCodec | 24 | PASS (1.16x) | 0.680 |

pub mod encoder;
pub mod decoder;
pub mod sequiv;
pub mod benchmark;
pub mod packed_codec;
pub mod error;

pub use encoder::{Encoder, EncodeResult, ThoughtVector, BaselineEncoder};
pub use decoder::{Decoder, DecodeResult, BaselineDecoder};
pub use sequiv::{SemEquiv, EquivResult, EquivLevel};
pub use benchmark::{SemanticRoundTripResult, bench_semantic_roundtrip, AntiCheatGates, bench_semantic_roundtrip_gated};
pub use packed_codec::{PackedCodecEncoder, PackedCodecDecoder, PACKED_CODEC_DIM};
pub use error::LatentError;
