//! # factum-mcp
//!
//! MCP (Model Context Protocol) bridge for Factum.
//!
//! Implements JSON-RPC 2.0 with:
//! - `initialize` handshake (includes morpheme table negotiation)
//! - `tools` capability: 8 tools (query, lookup, insert, insert_batch, upsert, assert, search, retract)
//! - `resources` capability: `factum://nodes/{id}`
//!
//! Resource subscriptions are not exposed by this stdio bridge.
//!
//! ## Token Optimization
//! Plain MCP clients receive canonical Factum text. Factum-aware clients can
//! negotiate compact output and receive its morpheme table during initialization.

pub mod protocol;
pub mod handler;
pub mod tools;

pub use protocol::*;
pub use handler::McpHandler;
pub use tools::ToolDefinition;
