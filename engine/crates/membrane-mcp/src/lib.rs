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
pub use tools::{install_executor, NativeMcpExecutor, tool_result, validate_arguments};
pub mod host_capability_matrix;
pub mod host_candidate_set;
pub mod host_continuity;
pub mod host_evidence_interceptor;
pub mod host_observable_event;
pub mod host_observable_ingress;
pub mod host_delivery_receipt;
pub mod host_delivery_ledger_store;
pub mod host_push_tool_egress;
pub mod host_context_adapter;
// Native host-adapter modules own this surface; legacy `mcp/host/index.mjs` is a
// pure re-export barrel (`export * from "./capability-matrix.mjs"` etc. for
// capability-matrix, candidate-set, continuity, evidence-interceptor). Its
// Rust equivalent is this file's `pub mod host_*` list above plus each
// module's own `pub use`/`pub fn` surface — no separate `host_index.rs` file
// is warranted.
