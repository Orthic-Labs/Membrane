//! Native JSON-RPC MCP surface for Membrane.
mod discovery;
mod jsonrpc;
mod tools;

pub use discovery::{discovery_response, initialize_response};
pub use jsonrpc::{serve_stdio, McpServer};
