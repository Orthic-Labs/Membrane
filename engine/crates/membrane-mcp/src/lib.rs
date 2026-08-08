//! Native JSON-RPC MCP surface for Membrane.
mod discovery;
pub mod http_security;
mod jsonrpc;
mod prompts;
mod resources;
mod tools;

pub use discovery::{discovery_response, initialize_response};
pub use jsonrpc::{serve_stdio, McpServer};
pub use prompts::{
    get_payload as get_prompt_payload, list_payload as list_prompts_payload, NAMES as PROMPT_NAMES,
};
pub use resources::{
    list_payload as list_resources_payload, read_payload, read_payload_by_name,
    read_result_payload, ReadOutcome, NAMES as RESOURCE_NAMES, URIS as RESOURCE_URIS,
};
