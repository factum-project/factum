//! # factum-mcp
//!
//! MCP (Model Context Protocol) bridge for Factum.
//!
//! Implements JSON-RPC 2.0 with:
//! - `initialize` handshake (includes morpheme table negotiation)
//! - `tools` capability: `factum_query`, `factum_insert`, `factum_retract`
//! - `resources` capability: `factum://nodes/{id}`
//! - `subscriptions`: mapped to factum-rt's `watch`
//!
//! ## Token Optimization
//! Default payload format is compact (numeric tags).
//! Morpheme index table is sent during `initialize` negotiation,
//! so subsequent messages use u32 indices instead of names.
//! This directly addresses the MCP token bloat problem.

pub mod protocol;
pub mod handler;
pub mod tools;

pub use protocol::*;
pub use handler::McpHandler;
pub use tools::ToolDefinition;
