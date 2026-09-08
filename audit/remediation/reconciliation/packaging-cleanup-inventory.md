# Packaging cleanup inventory

## Required fields

- status: complete
- summary: Current release still stages a bundled Blueprint Node tree plus Windows launch shims. Native Blueprint crate exists, but package/runtime policy, invocation graph, SBOM inputs, inventory checks, and seal verifier still encode Blueprint as external interpreter component.
- acceptance: Package, invocation, runtime-policy, SBOM, gate delta, and interpreter dispositions are cited below; no product/generated files changed.
- artifacts: `audit/remediation/reconciliation/packaging-cleanup-inventory.md`
- changes: Created only this report.
- commands: Read-only `Get-Content`, `rg`, PowerShell JSON projection; no tests/builds/generators/installs/Cargo/qualification.
- recovery: None.
- deviations: Package-size delta not measured because lane forbids builds/package generation and no comparable artifact was supplied; measure in final package qualification.
- blocker: None for inventory; later implementation must port/delete Blueprint Node payload before native-only package gates close.
- next: Integration owner applies paths below, then runs prescribed qualification.
- baselineRevision: `2e8e57bbb1e368332b37466080f1e925f2f4726e`
- citations: Dispatch receipt `audit/remediation/blueprint-native-reconciliation.dispatch.receipt.json:1-51`; packet `audit/remediation/blueprint-native-reconciliation.dispatch.json:161-184`; migration spec `migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md:33-38,60,85-97,101-109,565-610,650-724`.

## Package/invocation inventory

| Surface | Current evidence | Required later delta | Exact later paths |
|---|---|---|---|
| npm package | `blueprint/package.json:28-34` requires Node & exposes four Blueprint bins; `:38-68` ships scripts/watchman/src/mcp/providers/service. | Remove backend bins/runtime authority; retain only justified schemas/fixtures/docs or supported SDK. | Write `blueprint/package.json`; delete `blueprint/pnpm-lock.yaml` when no supported SDK remains; delete obsolete `blueprint/scripts/**`, `blueprint/src/**`, `blueprint/watchman/**` after parity.
| Launchers | `blueprint/release/launchers/blueprint:1-4` & `blueprint.cmd:1-5` exec `lib/node` + `blueprint.mjs`; MCP launchers do same. | Delete all Node shell/cmd shims; native `membrane blueprint ...` owns compatibility. | Delete `blueprint/release/launchers/blueprint`, `blueprint/release/launchers/blueprint.cmd`, `blueprint/release/launchers/blueprint-mcp`, `blueprint/release/launchers/blueprint-mcp.cmd`.
| Standalone archive | `blueprint/release/linux/blueprint-archive.json:2-15` names `bin/*`, `lib/node`, `app/package`; macOS distribution metadata is `blueprint/release/macos/distribution.xml:3-15`. | Remove standalone Node product or rewrite as native-only distribution. | Delete/rewrite `blueprint/release/linux/blueprint-archive.json`, `blueprint/release/macos/distribution.xml`, `blueprint/release/macos/postinstall`, `blueprint/release/macos/uninstall.sh`, plus related `blueprint/release/{README.txt,catalog.template.json,compatibility.template.json,update-manifest.template.json}`.
| Hub staging | `apps/membrane-hub/scripts/stage-runtime.mjs:1-20` recreates `src-tauri/runtime`, then spawns Node to `blueprint/scripts/release/stage-runtime.mjs`, staging `runtime/blueprint`. | Stage native sidecars/contracts only; no Blueprint Node subprocess/tree. | Write `apps/membrane-hub/scripts/stage-runtime.mjs`; delete obsolete `blueprint/scripts/release/stage-runtime.mjs`; update `apps/membrane-hub/scripts/runtime-inventory.mjs`.
| Hub inventory | `apps/membrane-hub/scripts/runtime-inventory.mjs:1-3,69-80` calls Blueprint an installed component & records named-pipe contract; checks at `:310-312` require Blueprint tree digest/file count. | Replace tree checks with native Blueprint binary/contract identity while preserving axis/contract checks. | Write `apps/membrane-hub/scripts/runtime-inventory.mjs`; generated `apps/membrane-hub/src-tauri/runtime/runtime-inventory.json` changes only during later package generation.
| Bundle/installer | `apps/membrane-hub/src-tauri/tauri.conf.json:6-10` bundles `runtime` & versions; Windows overlay adds native sidecars at `tauri.windows.conf.json:2-10`; NSIS extracts resources at `apps/membrane-hub/src-tauri/windows/installer.nsi:309-319`. | Reject `runtime/blueprint`, `lib/node`, legacy launchers; retain native binaries/contracts. | Write `apps/membrane-hub/src-tauri/tauri.conf.json`, `apps/membrane-hub/src-tauri/tauri.windows.conf.json` if native Blueprint delivery needs explicit entry; write `apps/membrane-hub/src-tauri/windows/installer.nsi`.

## Runtime policy/manifest delta

`migration/native-rust/runtime-policy.json:4-23` is sealed yet permits Node/shell and lists four `sealedExternalInterpreterRows`: Blueprint scripts, src, watchman, and release launchers. Rules at `:583-621` mark these packaged & production-reachable `external-typed-service`; Blueprint remainder is dev-only. Manifest totals at `migration/native-rust/runtime-language-manifest.json:7-23` are 1,351 files/58 rows, 26 Node, 1 shell, zero production interpreter rows, four bounded external interpreter rows; Blueprint rows are the four IDs named above.

Required later changes:

- Write `migration/native-rust/runtime-policy.json`: remove Blueprint from `sealedExternalInterpreterRows`; add exact deleted selectors for `blueprint/scripts/**`, `blueprint/src/**`, `blueprint/watchman/**`, launchers, and obsolete archive paths after deletion receipts.
- Regenerate/write `migration/native-rust/runtime-language-manifest.json` after cutover. Target `productionInterpreterRows: 0`, `boundedExternalInterpreterRows: 0`; no packaged Blueprint Node/shell rows.
- Write `migration/native-rust/invocation-graph.json`: no installed entrypoint reaches Blueprint JS, `lib/node`, or launchers. Current checker explicitly exempts Blueprint external references at `scripts/ci/check-invocation-graph.mjs:160-175` and labels its external boundary at `:259-264`; remove exemption/comment and require native reachability.
- Reconcile `migration/native-rust/executable-ledger.json` & `migration/native-rust/legacy-ledger-reconciliation.json`; current ledger still records Blueprint direct-call/external protocol sites (`migration/native-rust/executable-ledger.json:10-11`).

## SBOM & gate delta

`scripts/release/evidence/sbom.mjs:10-15,25-29` currently treats pinned Blueprint runtime & `blueprint/pnpm-lock.yaml` as shipped SBOM inputs; `:139-143,197-210` binds SBOM to lockfile and installer bytes. Write `scripts/release/evidence/sbom.mjs` to remove Blueprint lockfile/components after supported-SDK review while retaining `engine/Cargo.lock`, installer digest binding, and fail-closed checksum gaps.

`apps/membrane-hub/scripts/release-check-candidate-windows.mjs:22-45` requires native executables, JS hook projections, legal files, skills, and any `runtime/` closure; `:52-70` binds release manifest/SBOM/provenance. Write same file to reject `runtime/blueprint`, `lib/node`, Blueprint launchers, and interpreter extensions while retaining evidence bindings.

`migration/native-rust/native-only-seal.json:15-21` binds runtime manifest & invocation graph. `scripts/qualification/issue-native-only-seal.mjs:178-200` currently requires exact four Blueprint rows. Write same file to require zero bounded external rows and remove expected Blueprint map.

`scripts/ci/check-lifecycle-conformance.mjs:125-135` checks explicit Hub-off operations/six axes, while `:115-122` rejects stale daemon-only prose. Add archive/process-tree/no-interpreter assertions via `scripts/ci/check-runtime-language-manifest.mjs`, `scripts/ci/check-invocation-graph.mjs`, `scripts/ci/check-lifecycle-conformance.mjs`, or new `scripts/ci/check-native-package-closure.mjs`.

## Interpreter-file disposition

| Path/prefix | Current disposition | Later disposition | Proof |
|---|---|---|---|
| `blueprint/scripts/**` | Packaged Node runtime | Delete backend/release scripts | Parity + installed no-Node gate + deleted selector.
| `blueprint/src/**` | Packaged Node runtime | Delete JS backend; move supported schemas/contracts to native/neutral owner | Native parity + MCP/federation/CLI closure + SBOM.
| `blueprint/watchman/**` | Packaged Node watcher | Delete after native watcher/reconciliation cutover | Watcher/drop-event/restart qualification.
| `blueprint/release/launchers/**` | Packaged shell/cmd shims | Delete all four | Invocation graph + archive gate.
| `blueprint/release/linux/blueprint-archive.json` | Declarative archive names Node layout | Delete/rewrite native-only | Release/SBOM/archive gate.
| `blueprint/release/macos/{distribution.xml,postinstall,uninstall.sh}` | Standalone interpreted-product packaging | Delete/rewrite only for supported native distribution | Installer/update/uninstall qualification.
| `blueprint/package.json` | Node manifest/bins/dependencies | Rewrite non-runtime metadata or delete | Package boundary + SBOM lockfile closure.
| `blueprint/pnpm-lock.yaml` | SBOM input for Node runtime | Delete when no supported SDK remains | Generated SBOM `generatedFrom` proof.
| `blueprint/scripts/release/**` | Node staging/release tooling | Delete obsolete builders; retain only explicit dev/eval tooling | Dev-only exclusion + no reachability.
| `apps/membrane-hub/src-tauri/runtime/**` | Untracked staged payload excluded from manifest by policy (`migration/native-rust/runtime-policy.json:39-40`) | Ensure no Blueprint Node tree; native/declarative only | Installed artifact/process-tree gate.
| `scripts/release/evidence/sbom.mjs` | Blueprint lockfile/component input | Rewrite inputs | SBOM schema/digest evidence.
| `apps/membrane-hub/scripts/stage-runtime.mjs` | Node invokes Blueprint stager | Rewrite native staging | Candidate package gate.
| `apps/membrane-hub/scripts/runtime-inventory.mjs` | Requires Blueprint tree metadata | Rewrite native delivery identity | Inventory gate.
| `scripts/ci/check-invocation-graph.mjs` | Blueprint external exemption | Remove exemption | Fresh graph/reachability gate.
| `scripts/qualification/issue-native-only-seal.mjs` | Requires four Blueprint rows | Require zero bounded external rows | Native-only seal.

## Baseline/target

Baseline: revision `2e8e57bbb1e368332b37466080f1e925f2f4726e`; packet digest `sha256:030f24925602b59dd7b4557f6f4d1e2dffc94b246c281177b8c24f74e1086bcc`; receipt packet hash `8ab6486fa2c3086e9ac9f49d69aa876af9710d2d0f4021b657b24a81f9f990ca`. Target installed artifact contains native Membrane/Blueprint binaries/libraries, declarative contracts/assets, and permitted Hub presentation only: no Blueprint Node runtime, launchers, Node lockfile/SBOM components, or production-reachable interpreter source. Measure package-size delta from identical release inputs in final qualification; no measurement was performed in this read-only lane.
