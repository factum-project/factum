//! # factum-core
//!
//! Core data model, lexer, parser, and serialization for the Factum language.
//!
//! ## Modules
//! - [`types`]: Core data types — Node, Morpheme, Predicate, Term, Literal
//! - [`lexer`]: S-expression tokenizer
//! - [`parser`]: Hand-written recursive-descent parser
//! - [`serialize`]: Canonical and compact serialization
//! - [`morphemes`]: Morpheme registry (seed + runtime registration)
//! - [`calibration`]: Confidence calibration policy (provenance-based defaults)

pub mod types;
pub mod lexer;
pub mod parser;
pub mod serialize;
pub mod morphemes;
pub mod calibration;

pub use types::*;
pub use lexer::{Lexer, Token, TokenKind, LexError};
pub use parser::{Parser, ParseError};
pub use serialize::{canonical, compact, Serializer};
pub use morphemes::{MorphemeRegistry, MorphemeDef, MorphemeKind, ProposalStatus};
pub use calibration::{
    default_confidence_for_provenance,
    derived_confidence,
    derived_confidence_default,
    confidence_band,
    check_confidence_band,
    DEFAULT_RULE_RELIABILITY,
    EMPIRICAL_MIN_SAMPLES,
};
