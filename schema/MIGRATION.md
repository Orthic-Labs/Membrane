# Orthic contract bundle — migration & deprecation

This file is the canonical migration/deprecation record for the `orthic` contract
bundle (`schema/bundle.json`). It is owned by `OR-CONTRACTS` and is the only
place where a contract version is retired or a deprecated shape is admitted.

## Deprecation window

| Deprecated shape | Replaced by | Removed no earlier than |
|---|---|---|
| `orthic.product-manifest.v1` (`statusEndpoint` with inline `authToken`/dynamic host) | `orthic.product-manifest.v2` (static, secret-free, `artifactDigest`) | the next minor Hub release after both Cortex and Membrane publish v2 manifests |
| `orthic.snapshot.v1` (open `items` of arbitrary `serde_json::Value`) | `orthic.snapshot.v2` (bounded, content-free item objects) | the next minor Hub release after both products publish v2 snapshots |

A deprecated shape is **parsed but never admitted as v2**: the validator
(`src-tauri/src/manifest_validate.rs`, `schema/validate.mjs`) refuses a v1
inline secret as a v2 manifest (`manifest_schema_invalid`) and refuses a v1
snapshot schema version (`snapshot_schema_unsupported`). There is no implicit
promotion.

## v1 → v2 migration rule

1. A publisher moves the live endpoint, the bearer token, and the PID/port/fence
   out of the static manifest into the inherited lifecycle channel
   (`orthic.lifecycle.v1` `hello` frame). The static manifest keeps only version,
   product identity/version, install root, argv, icon, `hubCompatRange`, and
   `artifactDigest`.
2. The publisher stamps the released add-on digest into `artifactDigest`
   (`sha256:<64 hex>`). The Hub checks it before launch and again in the
   lifecycle `hello` frame.
3. A snapshot publisher bounds every section to the v2 caps
   (`schema/snapshot.v2.schema.json`): 1–16 sections, ≤1000 closed bounded
   evidence-handle item objects per section, ≤8 named scalar/handle fields per
   item (no arbitrary maps), ≤512-byte string values (label ≤128, kind ≤64),
   and a total payload cap of 65 536 bytes enforced at the Hub read boundary
   (`hub_runtime::MAX_SNAPSHOT_BYTES`). Schema, Rust, and Node agree on one
   total-byte number.

## Unsupported-future refusal

An unknown *future* schema version, comparator operator, or grammar extension
fails **closed** and typed:

- manifest `schemaVersion != 2` → `manifest_schema_invalid`;
- snapshot `schemaVersion != 2` → `snapshot_schema_unsupported`;
- `hubCompatRange` with an unknown operator (`~`, `^`, …), a four-component
  version, or a non-numeric component → `manifest_hub_range_incompatible`
  (`evaluate_hub_compat_range` returns `false`);
- a lifecycle frame whose `lifecycleVersion != 1` is refused by the supervisor.

The Hub never guesses compatibility it cannot prove, and a future product can
widen the contract only by shipping a new bundle version and a new
`hubCompatRange` that the current Hub fails closed against until it is itself
upgraded.