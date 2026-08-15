---
orthic:
  document_id: orthic-sol-implementation-plan
  type: plan
  status: accepted
  effective_from: 2026-08-14
  scope:
    deployable_units: [orthic]
    branches: [canonical]
  canonical_for: [implementation-sequencing, implementation-status, suite-contracts, delivery-gates]
---

# Orthic implementation plan

## Live status — 2026-08-15

- O0/F-OR & all six Phase A source owners are complete.
- OR-INTEGRATION amended ownership from 2,449 to 2,470 paths with no collision, proved patch composition, & integrated current working-tree candidate.
- `pnpm test` (39/39), schema fixtures/bundle, native harness (22/22), & full RightKit Rust suite (121/121) pass.
- `release:doctor` remains blocked by RightKit managed `CARGO_HOME` conflict; workspace fix exists in unpublished `@rightkit/release` 0.2.62.
- O4 adoption, J assembly, N-MAC/N-WIN, Oracle, Orthic commit/push, parent pins, publication, cross-attachment, & remote verification remain.

Current receipts: workspace `tasks/evidence/orthic-suite/integration/orthic/**`. Local integration is not release completion. Historical receipts retain original authority hashes.

Authority: workspace [`SEAM-CONTRACT.md`](../docs/plans/orthic/SEAM-CONTRACT.md) owns every Cortex ↔ Membrane ↔ Orthic boundary. This file owns Orthic supervisor-side implementation, contract releases, add-on adoption, installer assembly, status, & delivery closure. Current code, contract artifacts, native installed receipts, Git state, & remote state own operational truth.

No other Orthic implementation plan is current. `EC-2026-08-11-orthic-hub-consolidated-contract.md` plus earlier Hub contracts are evidence/rationale only; their surviving obligations map into O0–O6.

## 1. Outcome & boundary

Ship one product-neutral Orthic app that:

- discovers valid first-party product manifests & renders one tab per active product;
- supervises Cortex & Membrane children through one authenticated fenced lifecycle;
- renders bounded content-free snapshots with typed degradation;
- bundles both signed product add-ons by exact digest while onboarding controls activation;
- builds each Mac/Windows installer once, then attaches byte-identical artifact to both product releases;
- proves quit, off, crash, update, rollback, uninstall, & ownership loss leave zero orphan children.

Orthic owns app shell, onboarding, tray, product discovery, lifecycle contracts, supervision, Hub state, update orchestration, installer assembly, & generic status rendering. It does not own repository watching/graph truth, context admission/fusion, memory/data stores, product schemas/legal truth/qualification, product runtime builds, key bytes, or product-specific UI logic.

### Full-scope dispatch rule

Every O0–O6 obligation remains binding. Workspace [`tasks/plan.md`](../tasks/plan.md) compiles them into file-exclusive dispatches for minimum elapsed time; it changes no contract, security gate, native qualification, cross-attachment, or delivery state. O0 plus exact wire/file contracts freeze before source effects. O1–O4 file owners then execute concurrently against that packet; O5 native hosts run concurrently after exact-digest assembly; O6 remains integration-owner closure.

No production/test/config path may appear in two dispatch ownership sets. Generated outputs belong to generator owner. A new or omitted path must be assigned by integration owner before edit. Repair returns to same owner; no cleanup dispatch reopens another owner's file.

## 2. Current truth

Baseline revision: `ea800a97ba1c5fae6d813d268556af39e302ff10`; re-freeze tree in O0.

| Surface | Current classification | Required closure |
|---|---|---|
| Migrated Tauri chassis, brand, onboarding, manifest scan, tabs | Landed | Product-neutral fixtures, exact contract conformance, native installed proof |
| `manifest.v1` with inline `authToken`/dynamic endpoint | Unsafe legacy | Replace with static owner-only `orthic.product-manifest.v2`; secrets/live state move to lifecycle channel |
| `snapshot.v1` arbitrary `items` | Underbounded | Replace with bounded content-free `orthic.snapshot.v2` |
| Supervisor | Partial | Remove Membrane-specific env branch; honor declared stop or delete field; add authenticated hello/ready, fence, drain, backoff, update handoff, parent-loss, child-tree cleanup |
| Compatibility | Parsed, not proven semantic | Evaluate `hubCompatRange`, contract versions, artifact digest, & unsupported future versions before launch |
| Bundle | Membrane-only staging/resources | Adopt both Cortex & Membrane signed add-ons by exact digest; no product source build/path dependency |
| Initial status | May remain `unavailable` | Lifecycle endpoint registration drives `starting → ready/degraded/incompatible/failed` |
| Release lane | Landed scaffolding | One native installer build per platform, byte-identical cross-attachment, installed update/rollback proof |

## 3. Locked contracts

1. `orthic.product-manifest.v2` is static metadata: version, product identity/version, install root, argv, icon, `hubCompatRange`, artifact digest. It contains no token, port, PID, fence, instance ID, or live endpoint.
2. `orthic.lifecycle.v1` uses inherited authenticated pipe/channel. Hello binds installation/product/instance/artifact/fence identity; child registers endpoint + ephemeral capability. States, drain, stop, update handoff, ownership-loss acknowledgement, timeouts, & failures are closed enums.
3. `orthic.snapshot.v2` is read-only, loopback-authenticated, bounded, content-free, freshness-bearing, omission-bearing, & typed-degraded. Schema caps sections, items, strings, evidence handles, & total bytes.
4. Released contract bundle has canonical serialization, schema files, generated types, compatibility fixtures, version, digest, changelog/migration rule, & unsupported-future refusal. Product CI pins bundle; sibling-source fallback is prohibited.
5. `hubCompatRange` uses one documented semantic range grammar & fail-closed evaluator. Artifact digest is checked before launch & again in lifecycle hello.

## 4. Store & secret separation

| Root | Owner | Orthic access |
|---|---|---|
| Cortex SQLite/WAL/cache/temp/backup | Cortex | Published API only |
| Membrane/Crypt DB/index/cache/backup | Membrane | Opaque snapshot/evidence handles only |
| OS-keystore key material | OS + product key lifecycle | Opaque handle transport only; never key bytes |
| Hub manifests/leases/update journal/preferences | Orthic | Exclusive Hub state; never product data |

No shared SQLite, WAL, journal, cache, temp, backup, or mutable data root is allowed.

## 5. Execution program

| Package | Output | Dependency | Status |
|---|---|---|---|
| O0 Truth freeze | Current contract/code/release ledger, exact failing fixtures, dirty-tree classification | none | COMPLETE |
| O1 Contract v2 bundle | Manifest v2, lifecycle v1, snapshot v2, generated types, digest, compatibility matrices | sealed execution packet | INTEGRATED_UNCOMMITTED |
| O2 Generic supervisor | Product-neutral spawn/stop, handshake, readiness, fence, drain, crash loop, update handoff, child-tree cleanup | sealed execution packet | INTEGRATED_UNCOMMITTED |
| O3 Product-neutral shell | Dynamic tabs/severity, bounded snapshots, dormant activation, no product-specific branches/renderers | sealed execution packet | INTEGRATED_UNCOMMITTED |
| O4 Dual add-on adoption | Cortex + Membrane exact-digest staging, artifact verification, no source build/path deps | sealed product artifact digests | MECHANICS_GREEN; ADOPTION_PENDING |
| O5 Native lifecycle qualification | Mac/Windows install, first run, off/quit/crash/update/rollback/uninstall, zero orphan proof | O2 + O3 + O4 | PENDING_O4 |
| O6 Release closure | One installer built once, checksum-identical cross-attachment, receipts, nested push + parent pin | O5 | PENDING_O5 |

## 6. Package requirements

### O0 — truth freeze

1. Record revision/tree digest, current schemas/types, manifests, resource bundle, supervisor branches, stop behavior, compatibility handling, child census, listeners, permissions, & release scripts.
2. Freeze adversarial fixtures for schema/version/range/digest/mode/path/symlink/secret fields, lifecycle states, snapshot bounds, crash loops, update handoff, & process cleanup.
3. Map every surviving unit from historical consolidated contract to O1–O6; no old status carries forward.

### O1 — released contracts

1. Publish all §3 schemas/types/fixtures as one content-addressed artifact.
2. Migrate v1 only through explicit compatibility adapter; never accept v1 inline secrets as v2.
3. Define exact owner-only mode rules per host, semantic range grammar, payload caps, timeout bounds, state transitions, error/remediation codes, & deprecation window.

### O2 — supervisor

1. Replace product-ID branching with manifest + lifecycle data.
2. Create inherited authenticated channel without secret in manifest, argv, logs, crash dump, or snapshot.
3. Honor declared stop argv or remove declaration; after bounded drain, terminate full child tree.
4. Enforce monotonic fence, one owner, artifact digest, readiness deadline, capped restart/backoff, crash-loop state, update handoff, parent-death exit, & zero orphan census.

### O3 — shell

1. Render tabs & worst-state tray severity only from validated bounded snapshots.
2. Preserve typed `unavailable`, `degraded`, `incompatible`, `stale`, & omissions; never fabricate zero/healthy.
3. Keep product-specific details inside product surface. Hub renders generic sections/evidence handles only.

### O4 — add-on adoption

1. Consume signed Cortex & Membrane release artifacts, verify platform/architecture/version/digest/provenance, then stage without compiling product source.
2. Fail build when either required runtime/digest is absent. Both runtimes bundle; product picker controls activation, not download.
3. Record exact artifact digests in installer manifest & installed Hub state.

### O5/O6 — qualification & release

1. Run only-Cortex ⇒ one tab; both active ⇒ two tabs; Membrane activation implies Cortex; dormant runtime does no work.
2. Prove start/ready/degraded/stop/quit/off/crash/restart/update/rollback/uninstall on native Mac & Windows, including descendants, rogue endpoints, stale fence, old child, wrong digest, & incompatible range.
3. Build/sign/seal through RightKit on native host. Build installer once per platform; attach exact same bytes/checksum to Cortex & Membrane releases.
4. Push Orthic nested commit before parent pin. Parent gitlink is workspace history only; runtime compatibility uses contract versions/ranges & artifact digests.

## 7. Release gates

- zero bearer secret/dynamic endpoint in static manifest, argv, log, snapshot, or diagnostic bundle;
- zero product-specific supervisor branch or fixed Membrane section set;
- zero product source build/path dependency;
- exact contract bundle version/digest accepted by both products; unsupported future versions refuse typed;
- both signed add-ons adopted by exact digest;
- manifest/snapshot size & cardinality caps hold under adversarial input;
- Hub quit/off/update/uninstall yields zero orphan child/descendant on Mac & Windows;
- one installer per platform is byte-identical across both product release attachments;
- Orthic contracts released → product conformance → add-on adoption → installer built once → native installed proof closes in order.

## 8. Change control

Change workspace seam first for cross-product ownership/interface changes. Change this file for Orthic sequencing/status/gates. Change product plans for child-side implementation only. Add no parallel implementation plan; attach receipts under existing evidence/run paths.
