# Installed public Pull, recovery & tray proof — 2026-09-22

## Boundary

Internal unsigned Windows qualification passed. Fresh Codex invoked public Pull,
received Ledger evidence while Blueprint was stale, then recovered an exact
source span through public Pull. This is installed-path proof, not RELEASED
qualification or whole-product lifecycle closure.

Machine-readable assertions & identities: [proof.json](proof.json).
Actual installed desktop capture: [tray.png](tray.png).

- Clean runtime source: `736aa5fcd517f90a545b9cd5f05228c2a5de1fea`.
- Preceding public resolver, freshness & tray repairs: `71ded6d52b29037856b06af07f00525b786eac85`.
- Version: `0.1.24`.
- Generation: `sha256:caa07153256b01fb6812d57574b03e8a03ed12e293e6042c9863be74fce663ac`.
- Installer SHA-256: `ec13f795aad4e8cdc679d6f53d4ff2312a4bde9efe7a0b28437d06e2fdba5cdc`.
- Installed engine SHA-256: `820b90774f1d855118c3779a03f464c619bed132393f0969cc4a6b1464473ff8`.
- Installer-owned stable `current` resolves to `versions\0.1.24`.

## Production repairs

- Stale Blueprint evidence remains quarantined while independently identified
  Ledger/Cortex candidates retain their own eligibility checks.
- Ledger document & skill reads previously shared mutable SQLite TEMP query
  scopes. A budget-aware read-operation lock now protects scope, erasure view &
  cancellation handler for each read. Maintenance still uses its separate writer.
- Public `pull(operation: "source_read")` projects the owner-issued resolver;
  caller, ticket, source/span hashes, revision & generation stay intact.
- Empty native Pull responses retain rejection receipts. Activity records count
  actual delivered blocks; planned-but-undelivered blocks are not admissions.
- Windows caller-root comparison accepts equivalent slash/device-prefix forms
  while retaining distinct-root rejection.
- Tray separates engine status, last Pull outcome, state age & snapshot update.
  It uses the existing transparent icon, removes the redundant status square,
  labels budget drops explicitly & describes shared engine ownership correctly.

## Installed results

`pnpm run release:local:win:unsigned` passed through native Windows desktop.
Build/install/qualification/restore elapsed 26m32s. Installer receipt covers
startup, health, MCP, host cutover, tray/popup, renderer, Hub-owned & Hub-off
Blueprint, file-change refresh, repair/upgrade, state continuity, Adapt probes,
workspace migration, doctor, uninstall & residue. Downgrade was not applicable.

| Lifecycle case | Observed result |
|---|---|
| Graceful final owner | `graceful_replace_signal` → `final_holder_release`; 319 ms drain |
| Forced owner loss | `final_holder_expired`; 20,957 ms including lease expiry |

An earlier candidate's forced-loss qualification timed out while this agent's
own tool probes renewed a harness lease. Final run used passive log monitoring
through lifecycle checks; no timeout, ownership rule or acceptance gate was weakened.

Fresh Codex task `01a0c559-d8b4-7883-b41d-a4ad0ee7b1fa`, model `gpt-5.6-luna`:

1. Public Pull returned 12 Ledger blocks with typed `blueprint_stale` degradation.
   Complete serialized MCP result measured 23,925 tokens under declared 24,000.
2. Public Pull source recovery succeeded using returned resolver arguments.
   Result measured 627 tokens under the same budget. Recovered 149 bytes from
   `docs/architecture/execution-lifecycle-boundary.md`; source & span hashes plus
   byte equality against `[13778,13927)` were independently asserted.
3. Host capacity remained explicitly unobserved; no whole-transcript-fit claim.

An earlier test agent added an unauthorized outer `tool` argument to source
recovery; runtime rejected it with `context_envelope_invalid`. The successful
run passed only the resolver's returned `arguments` plus explicit budget fields.
Membrane hooks completed; a separate Arcane Stop completion hook reported failure.
This receipt claims observed Membrane calls, not an all-plugin completion result.

Final capture shows 24 admissions, matching two successful 12-block context
requests in the canonical runtime catalog. Source recovery does not add a context
admission. Counters summarize final admission receipts; upstream provider
omissions remain separately recorded in Pull packets. Engine readback was healthy,
watcher coverage complete & catalog healthy. Daily analysis remained unavailable.
The initial screenshot helper incorrectly treated `--activate` as a popup-open
command; final capture used the existing qualification tray-click callback.

## Verification

- Focused managed Rust checks: 7/7, including concurrent read scope isolation,
  maintenance independence, qualified activation binding, Windows path identity,
  non-delivery receipts & real admission counters.
- `pnpm test`: 32 passes, 1 intentional skip; restored suite 68 passes.
- `pnpm test:mcp`: 95 passes.
- Earlier focused public resolver tests covered exact bytes, scope/hash denial,
  tiny response budgets & invalid H8; this installed run adds the actual Codex path.

## Atom disposition

| Atoms | Evidence added | Still required |
|---|---|---|
| MEM-016 | Truthful installed tray/status & identity readback | RELEASED boundary & remaining promised-host proof |
| MEM-055, MEM-056 | Shared installed engine, graceful drain & forced lease expiry | RELEASED boundary & broader host ownership matrix |
| PUL-027, PUL-031 | Useful bounded delivery during stale Blueprint; measured wire result & admission accounting | Full acceptance matrix at RELEASED boundary |
| PUL-042 | Fresh Codex discovers/calls public exact Ledger resolver | Resolver negotiation/revocation/restart negative matrix at required boundary |
| PUL-050 | Actual-host bounded-response with explicit unknown host capacity | Complete host-fit & supported-transport qualification |
| LDG-022 | Installed native Ledger provider → Pull → exact public recovery | Full installed harness-only source path & RELEASED qualification |

Scope remains required or excluded. Counts remain 299 committed, 54 excluded &
0 lifecycle-closed. These results advance required qualification; they neither
remove remaining work nor reclassify it as deferred.
