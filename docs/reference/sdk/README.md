# Membrane SDK contract

Python, TypeScript, & Rust clients expose only `membrane_context` plus
`membrane_source_read`. Each client takes an injected transport; none opens a
socket, retries, starts, stops, or discovers a daemon.

Every response must match canonical v1 envelopes in
`schemas/registry/operations/membrane-*.v1.golden.json`: `schemaVersion: 1`,
`errorVersion: 1`, matching operation, & a closed success or typed-error result.
Unknown fields, versions, operations, & error codes fail closed.

```js
import { MembraneClient } from "@membrane/membrane-client";

const client = new MembraneClient((operation, request) => transport(operation, request));
const context = client.context({ task: "inspect" });
```

`ProtocolError` exposes `code`, `message`, `retryable`, & `details`; values
match Python client behavior. Rust exposes matching constants, `ProtocolError`,
& boxed transport through `MembraneClient::new`.

## Compatibility test suite

All three clients validate the identical canonical v1 golden envelopes in
`schemas/schemas/registry/operations/*.golden.json` and the identical closed error-code
sets in `schemas/registry/operations/operations-index.v1.golden.json`, so drift
between languages or against the shared schema fails a test instead of
requiring review to notice:

- Rust: `engine/crates/membrane-client/tests/compat.rs` (`rightkit cargo test
  --manifest-path engine/Cargo.toml -p membrane-client`).
- TypeScript: `tests/sdk/cross-language-compat.test.mjs` (`node --test
  tests/sdk/cross-language-compat.test.mjs`).
- Python: `tests/sdk/python_client_test.py` (`python3 -m unittest
  tests/sdk.python_client_test` from the repository root, or `python3
  tests/sdk/python_client_test.py`).

## Python packaging boundary and daemon-version compatibility

MBR-909 adds two further Python-only test files that do not have Rust/
TypeScript equivalents because they check packaging concerns specific to
publishing `membrane-client` on PyPI as a standalone distribution:

- `tests/sdk/python_package_boundary_test.py` proves the package
  (`dist/packages/python/`) is a thin client only — no core app, no other SDK,
  no native binary — against the machine-checkable declaration in
  `dist/packages/python/package-boundary.v1.json` /
  `schemas/sdk-python-package-boundary.v1.schema.json`. See
  `docs/reference/sdk/python-publishing.md`.
- `tests/sdk/python_daemon_compatibility_test.py` proves the client accepts
  every daemon receipt-schema version `schemas/context-receipt.v1.schema.json`
  currently lists as supported (today: `1` and `2`, i.e. the previous and
  current supported daemon) and fails closed against a newer, unsupported
  one. See `docs/reference/sdk/python.md`'s "Daemon compatibility" section.

## HTTP transport

`docs/reference/sdk/http-transport.md` documents the wire contract for injecting a
transport function that speaks the optional authenticated loopback
Streamable HTTP listener MBR-306 adds
(`engine/crates/membrane-runtime/src/mcp_http.rs`), including its current
`tools/call` dispatch limitation.
