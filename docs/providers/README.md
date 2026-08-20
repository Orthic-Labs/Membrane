# Membrane Provider SDK and Conformance Kit

MBR-104 seeds the contract surface that every Membrane-compatible external
adapter (Blueprint, Crypt, custom client adapters) implements and is validated
against. The SDK, the fixture corpus, and the two reference adapters are
the three artifacts that together establish a single, byte-exact Membrane
provider contract.

## Layout

| Path | Purpose |
|---|---|
| `engine/crates/membrane-provider-sdk/` | The Rust crate every adapter depends on. Defines the `Provider` trait, the `Fixture` shape, and the `run_conformance` harness. |
| `engine/crates/membrane-testkit/` | The canonical Blueprint and Crypt conformance fixture corpus, embedded as JSON and exposed via `golden_fixtures()`. |
| `docs/examples/providers/blueprint_example/` | Reference Blueprint adapter built on the SDK. |
| `docs/examples/providers/crypt_example/`  | Reference Crypt adapter built on the SDK. |
| `docs/providers/` | This README plus per-example docs. |

## The `Provider` trait

Every adapter implements three methods:

```rust
use membrane_provider_sdk::{Provider, ProviderError, Result, CapabilityV1};
use serde_json::Value;

pub trait Provider: Send + Sync {
    fn initialize(&mut self, config: &Value) -> Result<()>;
    fn list_capabilities(&self) -> Vec<CapabilityV1>;
    fn handle_operation(&self, operation: &str, request: &Value) -> Result<Value>;
}
```

* `initialize` is called once before any other method. The provider MUST
  be ready to serve `handle_operation` after a successful `initialize`.
* `list_capabilities` returns the operation names the provider supports.
  Each `CapabilityV1` carries the per-operation `schemaVersion` and
  `errorVersion` the registry in `membrane-protocol::operations`
  declares, so the adapter stays pinned to the same independent versions
  the runtime and the MCP tool surface use.
* `handle_operation` is the one entry point for every Membrane operation
  the provider advertises. The returned `Value` MUST be either a
  `{"kind": "success", "data": ...}` envelope or a
  `{"kind": "error", "code": "...", "message": "...", "retryable": <bool>,
  "details": <optional>}` envelope (the `OperationResult` discriminated
  union).

## The `Fixture` shape

A `Fixture` is a single canonical request/response pair the adapter must
round-trip through identically. The `membrane-testkit` crate embeds the
canonical Blueprint and Crypt fixture sets as JSON and exposes them via
`golden_fixtures()`. The same fixture set powers the SDK's own
conformance test (`engine/crates/membrane-provider-sdk/tests/conformance.rs`)
and the reference adapters' `run_*_conformance` helpers.

```json
{
  "name": "blueprint-context-scope-grant",
  "operation": "membrane_context",
  "description": "...",
  "request": { ... },
  "expectedResponse": { "kind": "success", "data": { ... } }
}
```

## The `run_conformance` harness

`run_conformance` takes one provider and one fixture set and asserts that
the provider produces the fixture's expected response for every fixture
in the set, byte-for-byte (after canonicalization). The harness also
sanity-checks that:

1. Every operation named by a fixture is also listed in
   `Provider::list_capabilities()`.
2. The response envelope is a valid `OperationResult` (discriminated
   `success` / `error` shape) so adapters cannot smuggle malformed
   output past the harness.

```rust
use membrane_provider_sdk::run_conformance;
use membrane_testkit::golden_fixtures;

let report = run_conformance(&my_provider, &golden_fixtures());
assert!(report.is_conformant(), "provider failed conformance: {:#?}", report.failed);
```

The report is JSON-serializable. Adapters that want to surface a
conformance failure outside the harness can convert any
`FixtureFailure` to a `ProviderError::ConformanceMismatch` via
`membrane_provider_sdk::mismatch_error`.

## Writing a conformant adapter

1. Add a dependency on `membrane-provider-sdk` (path
   `engine/crates/membrane-provider-sdk`).
2. Implement the `Provider` trait for your adapter struct.
3. Wire your adapter's conformance check to `membrane-testkit`'s
   `golden_fixtures()` (or a curated subset for adapter-specific
   regression suites).
4. Add a CLI entry point that runs `run_conformance` and prints the
   `ConformanceReport` to stdout. The reference adapters in
   `docs/examples/providers/{blueprint,crypt}_example/` are the canonical
   shape.

### What a fixture round-trip means

A fixture round-trips when the provider, given `fixture.request`,
returns a `serde_json::Value` whose canonical-JSON form is identical to
`fixture.expected_response`'s canonical-JSON form. Canonical-JSON is
the same byte contract `membrane-protocol::canonical::canonicalize`
exposes — sorted keys, no insignificant whitespace, no lossy coercion.
Adapters MUST therefore avoid producing values whose canonical form
differs from the fixture (e.g. through `serde_json::Value::Number`
coercion or by re-serializing a struct that uses different field
ordering than the fixture).

### Per-operation contract versions

The `membrane-protocol::operations` registry exposes one entry per
operation. Each entry carries:

* `schemaVersion` — the contract version of the operation. Bumping
  this MUST be a wire-incompatible change.
* `errorVersion` — the contract version of the operation's typed
  error taxonomy. Bumping this is wire-incompatible for callers that
  switch on closed error codes.

An adapter's `CapabilityV1` MUST carry the same `schemaVersion` and
`errorVersion` as the registry entry. The Book 1 gate will pin the
adapter's versions to the registry.

## What the deferred gate does

`cargo fmt --manifest-path engine/Cargo.toml --all -- --check` and
`cargo test --manifest-path engine/Cargo.toml --workspace` are the
manifest `deferredCommands` for MBR-104. They run at the Book 1 gate,
not in this task commit. This task's deferred-command posture is
identical to every other Book 1 task: implement, write tests and
fixtures, commit once, defer the gate.

## Reference adapters

* `examples/providers/blueprint_example/README.md`
* `examples/providers/crypt_example/README.md`
