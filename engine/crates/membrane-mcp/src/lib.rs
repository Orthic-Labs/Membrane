//! Native JSON-RPC MCP surface for Membrane.
mod discovery;
mod jsonrpc;
mod prompts;
mod tools;

pub use discovery::{discovery_response, initialize_response};
pub use jsonrpc::{serve_stdio, McpServer};
pub use prompts::{get_payload as get_prompt_payload, list_payload as list_prompts_payload, NAMES as PROMPT_NAMES};
