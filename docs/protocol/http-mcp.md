# Optional Streamable HTTP MCP

`membrane-mcp` defaults to stdio. HTTP has no listener in that crate.

A host opting into Streamable HTTP must call `http_security::admit` before
dispatch. Admission requires loopback peer plus loopback resolved host, exact
Host/origin allowlists, matching installation ID, nonempty bearer token,
constant-time comparison against policy-owned bearer & session bindings, bounded body, & nonzero bounded deadline. A non-loopback
host resolution is `dns_rebinding`; it is refused.

Receipt output is content-free: only `accepted`, typed `denial`, & transport.
It contains no token, session binding, request body, host, or installation ID.

## MBR-306: the listener

`membrane-runtime::mcp_http` (`engine/crates/membrane-runtime/src/mcp_http.rs`)
is the one place this optional transport is actually bound to a socket. It:

- Binds `127.0.0.1` only (`Ipv4Addr::LOCALHOST`), the same convention as the
  existing loopback Cortex API (`membrane-runtime::serve::run`).
- Exposes exactly one route, `POST /mcp`, carrying JSON-RPC request bodies.
- Re-resolves the request's `Host` header at request time (never cached) and
  denies as `dns_rebinding` on any non-loopback or failed resolution, so a
  host that answers loopback once and something else later is caught live
  rather than trusted from a stale answer.
- Calls `http_security::admit` with the real peer address, the freshly
  resolved host IP, and the request's headers/body before ever reaching
  `membrane_mcp::McpServer::dispatch` — the same dispatcher `serve_stdio`
  uses. This transport is an alternate, admission-gated entrypoint onto that
  one server, not a second implementation of it.
- Sources its bearer token from the same on-disk credential the loopback
  Cortex API already uses (`serve::configured_api_token`, i.e.
  `CORTEX_API_TOKEN` / `CORTEX_API_TOKEN_FILE` / the `api-token` file beside the
  database) rather than minting a parallel credential, and binds its session
  value to the resident's per-boot `StartupClaim::service_instance_id`.
- Logs only the bind address and port at startup. It never logs a bearer
  token, session binding, or request body.

**Not the default.** Nothing in `service::run_service` or `serve::run` (the
resident's default startup path) calls `mcp_http::run_mcp_streamable_http` or
`mcp_http::run_mcp_streamable_http_for_resident`. stdio
(`membrane_mcp::serve_stdio`, `serve::run_stdio_mcp`) remains the transport a
default resident install starts. A caller must explicitly invoke one of the
`mcp_http` entrypoints to open this listener at all — the transport exists
only for clients that cannot open a stdio pipe, not as a replacement default.
