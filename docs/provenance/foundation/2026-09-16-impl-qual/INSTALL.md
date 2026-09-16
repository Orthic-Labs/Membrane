# INSTALL lane report — impl qualification

Lane scope: installation, update, native-host projection, MCP/SessionStart
registration, activation/deactivation, and release-artifact paths:
`engine/crates/membrane/src/activation.rs`,
`engine/crates/membrane/src/modes.rs`,
`engine/crates/membrane-runtime/src/installation_manifest.rs`,
`engine/crates/cortex-store/src/installation_identity.rs`,
`apps/membrane-hub/scripts/{package-portable-windows,release-build-candidate-windows,release-check-candidate-windows,finalize-portable-release}.mjs`,
`apps/membrane-hub/src-tauri/windows/installer.nsi`,
host projection manifests at repo root (`.claude-plugin/`, `.codex-plugin/`,
`.antigravity-plugin/`, `.agents/`, `.mcp.json`, `hooks/`),
and the focused test rows under `apps/membrane-hub/tests/`,
`scripts/qualification/`, `tests/onboarding/`, `tests/clients/`.

No cargo/rightkit builds, installs, activations, or host-config writes were
performed: repository tooling denies direct Cargo locally
(`rightkit: cargo is GitHub Actions only`), so every Rust-side claim below is
static-inspection only and no atom is claimed `RELEASED`. Live-host proof
(actual Claude/Codex/Devin/etc. processes observing the projected surface)
was not executed in this lane.

Statuses: IMPLEMENTED+VERIFIABLE = mechanism confirmed by inspection and
covered by an in-tree executable test row; IMPLEMENTED-UNVERIFIED = mechanism
present but the atom's full behavior or released-boundary claim is not
verified by this lane; GAP-REPAIRED = a lane-owned gap was found and fixed
in-place; GAP-REMAINS = the atom's required end state is not met.

## ATOM TABLE

| Atom | Status | Evidence |
|---|---|---|
| MEM-018 installation manifest build/publish/verify | IMPLEMENTED+VERIFIABLE | `engine/crates/membrane-runtime/src/installation_manifest.rs:23-145` (runtime inventory, `installation_id`, schema/target binding); consumers `service.rs:515-516`, `serve.rs:2373-2388`; `release.json` file-hash closure written into the payload before `--payload-only` exit (`package-portable-windows.mjs:257`); qualification asserts exact sidecar inventory entries (`install-release.ps1:863-880`). Structural test rows: `install-release.test.mjs`, `portable-windows.test.mjs` — pass. |
| MEM-019 cloned-install detection, quarantine & identity rotation | IMPLEMENTED+VERIFIABLE | `engine/crates/cortex-store/src/installation_identity.rs` — clone/snapshot collision quarantines the colliding `installation_id` and rotates identity with reason `clone` (`installation_identity.rs:81`), identity file lock + atomic replace (`with_identity_lock`, `atomic_replace_prepared_if_fingerprint`); consumed via `installation_manifest.rs:5,96-143` → `service.rs:1034-1035` (`installationId`, `serviceInstanceId`). Test row: `engine/crates/cortex/tests/installation_identity.rs`. Not exercised against a live clone in this lane. |
| MEM-020 installed-state isolation from dev roots | IMPLEMENTED+VERIFIABLE | Test rows pass: `install-boundary.test.mjs` ("development Hub cannot become installed product or mutate global bindings", "retired install workspace projection cannot return"), `mem-windows.test.mjs`, `mem-lifecycle-windows.test.mjs`. Activation is stable-`current`-only: `activate_internal` resolves the install root through the `current` junction (`activation.rs:492+`); user PATH work is isolated to the install root (`ensure_isolated_user_path`/`without_path_entry` `activation.rs:1953-2210`). |
| MEM-021 transactional candidate staging + cryptographic admission | GAP-REPAIRED | NSIS stages `versions\<v>` then junction `.current-next` before touching live `current` (`installer.nsi:491-537`); candidate admission is digest-bound (`release-check-candidate-windows.mjs:44-48` archive size+sha256, `:53-57` release-manifest/SBOM archive binding). Repaired: `verify-version-tree` now fails the install unless the full host-projection surface (`release.json`, `plugin.json`, `mcp.json`, `mcp_config.json`, `.mcp.json`, `hooks/hooks.json`, `hooks/codex-hooks.json`, `.claude-plugin/{plugin,marketplace}.json`, `.codex-plugin/plugin.json`, `.agents/plugins/marketplace.json`, `.antigravity-plugin/{plugin,mcp_config}.json`) exists in the staged tree — previously only the five executables were checked, so a partial projection could reach `current`. Packager also repaired to include `membrane-daemon.exe` so the signed payload closure matches the candidate closure. |
| MEM-045 Claude native plugin (SessionStart + authenticated HTTP MCP) | GAP-REPAIRED | `.claude-plugin/plugin.json` + `marketplace.json`; `hooks/hooks.json` is the single hook source (inline plugin hooks removed — eliminates double registration); `.claude-plugin` MCP declares `Authorization: Bearer ${MEMBRANE_BEARER_TOKEN}` against `http://127.0.0.1:47851/mcp`; activation reconciles the plugin first and suppresses fallback settings-level hook/MCP duplication while it is enabled (`activation.rs:3303+`, `reconcile_host_plugins`); plugin state recorded per-client on the activation receipt (`activation.rs:146`). Fixture/static verification only: `getting-started-consistency.test.mjs`, `portable-windows.test.mjs` pass. Live Claude Code SessionStart + MCP initialize/list proof outstanding. |
| MEM-046 Codex native Agent Plugin | GAP-REPAIRED | `.codex-plugin/plugin.json` references `./hooks/codex-hooks.json` + `./.mcp.json`; `.mcp.json` uses `bearerTokenEnvVar: MEMBRANE_BEARER_TOKEN` (no token in argv/logs) — enforced post-stamp in both packagers (`package-portable-windows.mjs:174-188`, `release-build-candidate-windows.mjs:163-178`). Activation runs `mcp add membrane --url <installed MCP URL> --bearer-token-env-var MEMBRANE_BEARER_TOKEN` (`activation.rs:3882` region), CLI-first then state-verified; plugin-enabled state suppresses fallback registration. Live Codex host proof outstanding. |
| MEM-047 CodeRight L5 binding | IMPLEMENTED-UNVERIFIED | Mechanism present: `coderight-adopt` mode (`modes.rs:379,477-479`), CodeRight observation schemas (`host_observation_ingress.rs:19-23`), H4 execution schema (`adapt_efficiency_qualification.rs:14-15`). No lane repair needed or applied; the declared-L5 signed native HTTP binding has no in-tree executable acceptance row run in this lane and no live CodeRight proof. |
| MEM-048 Cursor/Windsurf stable `membrane-client stdio-mcp` projection | GAP-REPAIRED | Config-managed clients write exactly `{command: <installed membrane-client>, args: ["stdio-mcp"]}` into each client's MCP config, preserving foreign entries (`activation.rs` `reconcile_config_clients`/`deactivate_config_clients` `2835-2891+`); Devin added as a config-managed client this lane (see MEM-069). Deactivation removes only exact Membrane-owned entries. Static verification only; live Cursor/Windsurf client proof outstanding. |
| MEM-057 atomic activation of one admitted candidate | GAP-REPAIRED | Staged junction built as `.current-next`, live `current` renamed aside to `.current-previous`, staged junction renamed into place (`installer.nsi:491-537`); junction removal is non-recursive `RMDir` only — never recurses into a target (asserted `installer-nsi-activation-contract.test.mjs:84-99`). Repaired: `bind-installed-clients` now runs before OS registration so a failed reconcile precedes registry claims, and `$R5` cutover flag drives `.current-previous` restore in `install_failed` — post-cutover failures no longer strand a new `current` without working bindings. |
| MEM-058 previous-version retention & transactional rollback | GAP-REPAIRED | `.current-previous` is now retained until after bindings reconcile and registration succeed (dropped only at `install ${VERSION} complete`); `install_failed` restores it (`installer.nsi` install_failed block). Previously `.current-previous` was deleted immediately after cutover, so any post-cutover failure (register/bind) left the new tree live with no rollback path. Version trees under `versions\<v>` are retained; rollback of `current` is junction-rename only. |
| MEM-059 governed uninstall of runtime, tray & bindings | IMPLEMENTED+VERIFIABLE | Uninstall section: claims install lock, ends/deletes legacy supervisor task, runs `membrane.exe deactivate` (non-fatal, logged), removes `current`/`.current-next`/`.current-previous` junctions non-recursively with abort guards before any recursive delete, removes product root, registry uninstall/Run entries, shortcuts (`installer.nsi:680-760`). Structural rows pass (`installer-nsi-activation-contract.test.mjs:133-145`). Live uninstall proof not executed. |
| MEM-060 downgrade rejection without explicit authorization | IMPLEMENTED-UNVERIFIED | Policy declared: `!define RIGHTKIT_AUTOMATIC_IN_PLACE_UPGRADE` (`installer.nsi:21`), `allowDowngrades: true` at bundle level (`tauri.windows.conf.json`), qualification exercises downgrade-as-rollback (`install-release.ps1` `downgrade = $rollback`, "previous installer version not older than current"). Whether the rendered installer hard-rejects an unauthorized downgrade depends on the RightKit in-place-upgrade contract at makensis time — not inspectable/verifiable in this lane. |
| MEM-067 one bounded orientation packet via native SessionStart (Codex & Claude) | GAP-REPAIRED | `hooks/hooks.json` (Claude event superset) and `hooks/codex-hooks.json` (Codex event set) invoke `membrane-client` hook transport — never `membrane.exe` (enforced post-stamp in both packagers); `hook.rs`/`hook_diagnostics.rs` own the bounded packet; activation reconciles owned hook events per client (`reconcile_claude_hooks`/`reconcile_codex_hooks` `activation.rs:2391-2502+`) and removes fallback hooks when the native plugin owns registration. Fixture/static verification only; live hook-fire proof outstanding. |
| MEM-069 Devin independent projection | GAP-REPAIRED | Devin added as a config-managed `HarnessClient::Devin` (`activation.rs:52,63,77,88`): user-scoped MCP config at `%APPDATA%\devin\mcp_config.json` (Windows) / `~/.config/devin/mcp_config.json` (POSIX) (`activation.rs:2719-2732`), registered via installed `membrane-client stdio-mcp` — an independent projection, no capability inferred from another host. Deactivation removes only the exact Membrane-owned Devin entry. Static verification only; live Devin client proof outstanding. |

## CHANGES MADE

Manifests / descriptors:
- `hooks/hooks.json`, `hooks/codex-hooks.json` — client-transport hook commands (`membrane-client`), Claude event superset vs Codex event set.
- `.mcp.json` — Codex root MCP manifest with `bearerTokenEnvVar: MEMBRANE_BEARER_TOKEN`, `http://127.0.0.1:47851/mcp`.
- `.claude-plugin/plugin.json` — removed inline hooks (single hook source in `hooks/hooks.json`), bearer-env Authorization header; `.claude-plugin/marketplace.json`.
- `.codex-plugin/plugin.json` — references `./hooks/codex-hooks.json` + `./.mcp.json`.
- `.agents/plugins/marketplace.json`, `.antigravity-plugin/mcp_config.json` — projection surface.

Activation (`engine/crates/membrane/src/activation.rs`, `modes.rs`):
- `HarnessClient::Devin` + config-managed Devin path; `activation_scope` on `ActivationReceiptV1` (`full`/`engine_only`/`bindings_only`); `PluginActivationReceipt` + `plugin` field on `ClientActivationReceipt`; `ClientState::PluginOwned`; `reconcile_host_plugins` runs before fallback client reconciliation; plugin-enabled Codex/Claude suppress settings-level hook/MCP duplication; deactivation removes owned plugin projections and exact owned entries only; POSIX/macOS MCP credential provisioning + user PATH add/remove implementations replacing stubs.

Packaging / release:
- `package-portable-windows.mjs` — `hooksManifestPath` into `assemblePortableCore`, `.agents`/`.mcp.json`/`codex-hooks.json`/`.antigravity-plugin` copies, post-stamp payload-surface + transport-binding validation block, `membrane-daemon.exe` added to the executable closure.
- `release-build-candidate-windows.mjs` — same projection copies + post-stamp validation for the unsigned candidate.
- `release-check-candidate-windows.mjs` — authenticated MCP transport probe (`probeCandidateMcp`: engine launch, credential wait, HTTP `initialize`/`tools/list`, `membrane-client stdio-mcp` forwarding, bounded shutdown), spawn-error fast fail, `MEMBRANE_CANDIDATE_TRANSPORT_PROBE=0` explicit fixture opt-out reported as `"transportProbe": "skipped"`, plugin-surface closure check.
- `apps/membrane-hub/src-tauri/windows/installer.nsi` — post-overlay host-projection-surface validation in `verify-version-tree`; `bind-installed-clients` moved before `register`; `.current-previous` retained until install completes; `install_failed` restores the previous junction when cutover happened (`$R5`).
- `finalize-portable-release.mjs` — candidate handoff alignment.

Tests:
- `apps/membrane-hub/tests/candidate-handoff.test.mjs` — fixture covers new closure surface; probe opt-out explicit; probe-negative run now restores the tampered archive first so the failure is the probe, not the digest (pre-existing ordering bug fixed).
- `apps/membrane-hub/tests/portable-windows.test.mjs` — asserts transport probe + projection surface instead of `hook --help`.
- `apps/membrane-hub/tests/windows-release.test.mjs` — stale pre-migration assertions updated to the current contract (engine binary owns activate/deactivate, tray owns login launch; retry-label ordering; `membrane engine sidecar` naming).
- `tests/onboarding/getting-started-consistency.test.mjs` — separate hook-manifest expectations.
- `scripts/tools/productization/check-docs.test.mjs` — stale `toolCount === 23` assertion updated to 2: the public MCP registry was collapsed to `pull`/`push` (fixture + `tools.rs` + regenerated docs already landed by the MCP lane; `install-release.test.mjs` asserts `$publicTools = @('pull','push')`).

## TEST COMMANDS & OUTCOMES

- `node --test scripts/qualification/installer-nsi-activation-contract.test.mjs scripts/qualification/install-release.test.mjs apps/membrane-hub/tests/portable-windows.test.mjs` — 32/32 pass.
- `node --test apps/membrane-hub/tests/candidate-handoff.test.mjs` — 1/1 pass (fixture runs archive extract + digest/closure checks; transport probe explicitly `skipped`, probe-negative run fails closed on inert stubs).
- `node --test scripts/install-boundary.test.mjs scripts/qualification/cases/{mem-windows,mem-lifecycle-windows,ex-windows}.test.mjs tests/clients/client-matrix.test.mjs apps/membrane-hub/tests/{agents-adapters,windows-release}.test.mjs tests/onboarding/getting-started-consistency.test.mjs scripts/qualification/{installer-nsi-activation-contract,install-release}.test.mjs apps/membrane-hub/tests/{portable-windows,candidate-handoff}.test.mjs` — 107/107 pass.
- Rust builds/tests: not run — `rightkit`/`cargo` are GitHub-Actions-gated for this checkout; direct invocation denied. All `activation.rs` changes are static-inspection only pending managed CI compile + test.

## UNRESOLVED REQUIREMENTS / BLOCKERS

1. Rust compile + unit/integration verification of `activation.rs` (plugin reconciliation, Devin path, receipt fields, POSIX credential/PATH) requires the managed CI path; static inspection only here.
2. Live installed-host proof for Claude Code, Codex, Cursor, Windsurf, Antigravity, Devin (SessionStart hook fire + authenticated MCP initialize/tools/list against the resident listener) has not been executed — required before any `RELEASED` claim per the acceptance boundaries.
3. MEM-060: actual downgrade rejection is enforced by the RightKit in-place-upgrade contract rendered at makensis time; not provable from the template alone.
4. MEM-047: CodeRight declared-L5 binding has no executed acceptance row in this lane.
5. NSIS template cannot be compiled locally (`installer-nsi-compiles.test.mjs` documents this); NSIS edits are structurally pinned by tests, not makensis-verified.

## NEXT DEPENDENCY

Managed CI run (RightKit `cargo` on GitHub Actions) to compile `activation.rs` and run Rust test rows, then the installed-qualification harness (`install-release.ps1`) on a Windows host to produce the live installed-boundary evidence every host atom still lacks.
