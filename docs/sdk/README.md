# Membrane SDK contract

Python, TypeScript, & Rust clients expose only `membrane_context` plus
`membrane_source_read`. Each client takes an injected transport; none opens a
socket, retries, starts, stops, or discovers a daemon.

Every response must match canonical v1 envelopes in
`operations/operations/membrane-*.v1.golden.json`: `schemaVersion: 1`,
`errorVersion: 1`, matching operation, & a closed success or typed-error result.
Unknown fields, versions, operations, & error codes fail closed.

```js
import { MembraneClient } from "@orthic/membrane-client";

const client = new MembraneClient((operation, request) => transport(operation, request));
const context = client.context({ task: "inspect" });
```

`ProtocolError` exposes `code`, `message`, `retryable`, & `details`; values
match Python client behavior. Rust exposes matching constants, `ProtocolError`,
& boxed transport through `MembraneClient::new`.

## Compatibility test suite

All three clients validate the identical canonical v1 golden envelopes in
`operations/operations/*.golden.json` and the identical closed error-code
sets in `operations/operations/operations-index.v1.golden.json`, so drift
between languages or against the shared schema fails a test instead of
requiring review to notice:

- Rust: `engine/crates/membrane-client/tests/compat.rs` (`cargo test
  --manifest-path engine/Cargo.toml -p membrane-client`).
- TypeScript: `tests/sdk/cross-language-compat.test.mjs` (`node --test
  tests/sdk/cross-language-compat.test.mjs`).
- Python: `tests/sdk/python_client_test.py` (`python3 -m unittest
  tests/sdk.python_client_test` from the repository root, or `python3
  tests/sdk/python_client_test.py`).

## HTTP transport

`docs/sdk/http-transport.md` documents the wire contract for injecting a
transport function that speaks the optional authenticated loopback
Streamable HTTP listener MBR-306 adds
(`engine/crates/membrane-runtime/src/mcp_http.rs`), including its current
`tools/call` dispatch limitation.
