# Wiring a transport against the loopback Streamable HTTP listener

Every SDK client (`MembraneClient` in Rust, TypeScript, and Python) is
transport-injected only: it never opens a socket, never discovers a daemon,
and never retries. This document is the wire-level reference a caller needs
to write the transport function they inject when they choose to speak to the
optional authenticated loopback Streamable HTTP listener that
`engine/crates/membrane-runtime/src/mcp_http.rs` adds (MBR-306), instead of
stdio. It is descriptive of the real transport, not a second implementation
of it — the SDK packages ship no HTTP client and duplicate none of
`membrane_mcp::http_security`'s admission policy.

## The listener is opt-in, not the default

stdio (`membrane_mcp::serve_stdio`) remains Membrane's default MCP transport.
Nothing in the resident's default startup path opens the HTTP listener; an
operator must explicitly call `mcp_http::run_mcp_streamable_http` or
`run_mcp_streamable_http_for_resident`. See `docs/protocol/http-mcp.md` for
the full listener contract. This page only covers what an SDK-side transport
function must send.

## Request shape

- **Method / path:** `POST /mcp` (`membrane_runtime::mcp_http::MCP_HTTP_PATH`).
- **Body:** a single line-oriented JSON-RPC request object, the same framing
  `serve_stdio` reads line-by-line from stdin.
- **Headers, all required for admission** (`membrane_mcp::http_security::admit`,
  checked before the request ever reaches `McpServer::dispatch`):
  - `Host` — must exactly match the policy's allowed host (e.g.
    `127.0.0.1:<port>`). A hostname is re-resolved live on every request; an
    answer that is not loopback is denied as `dns_rebinding` rather than
    trusted from a cached lookup.
  - `Origin` — must exactly match the policy's allowed origin (e.g.
    `http://127.0.0.1:<port>`).
  - `Authorization: Bearer <token>` — the same on-disk credential the
    loopback Crypt API already uses (`serve::configured_api_token`). Compared
    in constant time; a missing header is `missing_bearer`, a wrong value is
    `invalid_bearer`.
  - `x-membrane-installation-id` (`membrane_runtime::mcp_http::INSTALLATION_HEADER`)
    — must equal the resident's installation id, or the request is denied as
    `installation_mismatch`.
  - `x-membrane-session` (`membrane_runtime::mcp_http::SESSION_HEADER`) — must
    equal the resident's per-boot `StartupClaim::service_instance_id`, or the
    request is denied as `session_mismatch`.
- The peer connection itself must originate from loopback
  (`non_loopback_peer` otherwise), the body must stay under the policy's
  `max_body_bytes` (`body_too_large` otherwise), and any client-declared
  deadline must be nonzero and under the policy's `max_deadline_ms`
  (`deadline_too_long` otherwise).

A denied request never carries the bearer token, session binding, or request
body back in its response — only `accepted`, a typed `denial`, and
`transport: "streamable_http"`, matching
`schemas/http-mcp-security.v1.schema.json`
(`HttpAdmissionReceiptV1`). Every denial code above is a literal value of
that schema's `denial` enum: `non_loopback_peer`, `dns_rebinding`,
`host_not_allowed`, `origin_not_allowed`, `installation_mismatch`,
`missing_bearer`, `invalid_bearer`, `session_mismatch`, `body_too_large`,
`deadline_too_long`.

## What admission does not cover: MCP `tools/call` wiring

Admission is transport plumbing, not the operation contract. Once a request
is admitted, `mcp_http` hands the parsed JSON-RPC body to
`membrane_mcp::McpServer::dispatch` — the exact same dispatcher `serve_stdio`
uses, so an admitted HTTP request and an equivalent stdio request produce an
identical JSON-RPC response. As of this change, that shared dispatcher's
`tools/call` branch (`engine/crates/membrane-mcp/src/jsonrpc.rs`) is a stub
that returns `{"content":[{"type":"text","text":"native_tool_execution_unsupported"}],"isError":true}`
for every tool call, regardless of transport. The canonical `membrane_context`
/ `membrane_source_read` operation envelopes this SDK validates
(`schemas/operations/membrane-context.v1.schema.json`,
`schemas/operations/membrane-source-read.v1.schema.json`) are not yet
reachable through a live `tools/call` round trip over either transport — only
through the golden fixtures in `operations/operations/*.golden.json` and
whatever in-process or test transport a caller injects directly. A transport
function built from this page is correct for the admission layer today;
treat the operation dispatch itself as not yet wired until that stub is
replaced.

## Symmetry across languages

The header names, path, and denial codes above are literal constants in
`engine/crates/membrane-runtime/src/mcp_http.rs` and
`schemas/http-mcp-security.v1.schema.json`.
`tests/sdk/http-transport-contract.test.mjs` reads both files at test time
and asserts this document still names the same path, header names, and
denial codes, so a rename in the transport source fails this documentation
mechanically instead of going stale silently.
