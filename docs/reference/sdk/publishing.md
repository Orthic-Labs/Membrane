# SDK crate publishing policy

MBR-908 makes `membrane-protocol` & `membrane-provider-sdk` independently
consumable Rust crates. They retain `license-file` metadata from repository
root & publish no application, runtime, installer, storage, or credentialed
release implementation.

## Compatibility boundary

- `membrane-protocol` owns public protocol types, canonical JSON helpers, &
  operation registry.
- `membrane-provider-sdk` owns `Provider`, `CapabilityV1`, fixtures,
  conformance reports, & provider errors.
- Compatible downstream providers declare the `0.1` semver requirement for
  both crates.
- Public changes follow SemVer; while major version is zero, breaking API or
  contract changes require the next minor release.

## Release gate

`engine/crates/membrane-provider-sdk/tests/downstream_fixture.rs` packages &
extracts both crates, then creates a standalone consumer package. Its
integration test runs `rightkit cargo check --offline` against those extracted,
versioned dependencies, proving a fresh downstream provider compiles without
workspace-only imports.

Publishing stays a separately authorized release action. This source change
does not publish either crate.
