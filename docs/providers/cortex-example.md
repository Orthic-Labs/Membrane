# `cortex_example` — reference Cortex adapter

A minimal but real reference adapter that demonstrates how a
Cortex-shaped Membrane provider is written against
`membrane-provider-sdk`.

## What it does

The `cortex_example` crate implements the `Provider` trait for two
operations from the Membrane registry:

| Operation               | Purpose                                                   |
|-------------------------|-----------------------------------------------------------|
| `membrane_context`      | Returns a workspace context with an active scope grant.   |
| `membrane_source_read`  | Returns a source-read envelope for one anchor section.    |

The response shapes mirror the per-operation JSON Schemas and golden
fixtures under `engine/crates/membrane-protocol/operations/`. The
adapter is deterministic: given a request envelope, it always returns
the same response (the one the corresponding `membrane-testkit`
fixture declares). The `membrane-provider-sdk` conformance test
(`engine/crates/membrane-provider-sdk/tests/conformance.rs`) exercises
this property via `run_conformance`.

## Layout

```
docs/examples/providers/cortex_example/
├── Cargo.toml      # member of docs/examples/providers/ workspace
├── README.md       # this file
└── src/
    ├── lib.rs      # the Provider impl + handle_* helpers
    └── main.rs     # the tiny CLI: cortex_example run-conformance
```

## How to run

From the repository root:

```sh
cargo run --manifest-path docs/examples/providers/cortex_example/Cargo.toml --bin cortex_example
```

The binary exits 0 if every Cortex fixture in `membrane-testkit` passes
the SDK's `run_conformance` harness, 1 otherwise. The output is the
JSON-serialized `ConformanceReport` on stdout.

## Notes

* The adapter is intentionally pure (no I/O, no clock, no randomness).
  A real Cortex adapter will read the cortex graph and the scope-grant
  store; those reads are out of scope for the conformance adapter —
  the SDK is the contract layer, not the implementation layer.
* The `CapabilityV1` entries carry `schemaVersion = 1` and
  `errorVersion = 1`, matching the entries in
  `membrane-protocol::operations()` for `membrane_context` and
  `membrane_source_read`.
* The reference adapter handles `UnknownOperation` by returning
  `ProviderError::UnknownOperation` rather than producing a typed
  error envelope. This is the correct shape: the harness distinguishes
  `Err(ProviderError::...)` from a typed `error` response.
