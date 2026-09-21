//! Error types for factum-l.

use thiserror::Error;

/// Errors that can occur during latent space operations.
#[derive(Debug, Error)]
pub enum LatentError {
    #[error("encode error: {0}")]
    EncodeError(String),

    #[error("decode error: {0}")]
    DecodeError(String),

    #[error("equivalence comparison error: {0}")]
    EquivError(String),

    #[error("dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    #[error("registry error: {0}")]
    RegistryError(String),
}
