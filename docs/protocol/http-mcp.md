# Optional Streamable HTTP MCP

`membrane-mcp` defaults to stdio. HTTP has no listener in this crate.

A host opting into Streamable HTTP must call `http_security::admit` before
dispatch. Admission requires loopback peer plus loopback resolved host, exact
Host/origin allowlists, matching installation ID, nonempty bearer token,
constant-time comparison against policy-owned bearer & session bindings, bounded body, & nonzero bounded deadline. A non-loopback
host resolution is `dns_rebinding`; it is refused.

Receipt output is content-free: only `accepted`, typed `denial`, & transport.
It contains no token, session binding, request body, host, or installation ID.
