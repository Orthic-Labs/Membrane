# MCP lifecycle semantics

`mcp/server.mjs` advertises MCP `logging`, handles client `logging/setLevel`, emits bounded structured lifecycle logs, reports progress only for a valid client progress token, & passes the request abort signal to context providers and workspace workers.

`membrane_working_context(operation=load)` accepts `limit` plus `cursor` for durable context history. Its cursor binds the last immutable `(created_at, context_id)` key, so appended rows do not duplicate an earlier page.

The native Rust MCP surface currently serves discovery, resources, & prompts only. It does not advertise or execute tool calls, so it deliberately does not advertise lifecycle logging or progress support.
