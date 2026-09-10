# MCP lifecycle semantics

The native Rust MCP server is implemented by `engine/crates/membrane-mcp/` &
dispatched by `engine/crates/membrane-runtime/src/mcp_executor.rs`. Its installed
stdio entrypoint is `membrane stdio-mcp`; native tool discovery & execution use
the registry in `engine/crates/membrane-mcp/src/tools.rs`.

`membrane_working_context(operation=load)` accepts `limit` plus `cursor` for durable context history. Its cursor binds the last immutable `(created_at, context_id)` key, so appended rows do not duplicate an earlier page.

The native Rust MCP surface serves tools, resources, & prompts. It does not
advertise lifecycle logging or progress support because those capabilities are
not part of its current JSON-RPC registry.
