# Membrane 0.1.24 — Daily Dogfood Checklist (Windows)

> Consolidated from the prior `docs/DOGFOOD.md`, `D:\Downloads\glm.md` (source-only
> review), and `D:\Downloads\solar.md` (source-only review), then re-verified live
> against the installed product at
> `C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current` and the repo at
> `D:\Claude\membrane` on 2026-09-03. Every check states the exact command/file and
> the exact expected value. Every claim below was either (a) verified live on this
> machine on 2026-09-03 — marked **VERIFIED**, or (b) could not be checked and is
> marked **UNVERIFIED** in §6. Claims from the source documents that turned out to
> be wrong are corrected inline and listed again in the final report.

---

## 1. What Membrane is and does

Membrane is a local-first context control plane between an agent and its sources.
Given a task it determines the smallest sufficient, current evidence set under a
budget/deadline, and returns it with a receipt showing what was included, omitted,
or degraded and why. It is a parent system with exactly six named subsystems (repo
invariant, `docs/agent-rules.md`, `docs/architecture/membrane.md`).

| Subsystem | Owns | Observable |
|---|---|---|
| **Pull** | Retrieve, admit, fuse, publish task-relevant evidence. | `membrane_context` returns a packet + receipt; unavailable providers degrade typed rather than fabricate. |
| **Push** | Faithful, reversible reduction of information already in flight. | No dedicated MCP tool; wired into the host hook loop. Protected spans (identifiers, exact errors, failing-test names, URLs, code fences) should survive reduction; truncation is marked, never silent. |
| **Cortex** | Governed durable-memory admission, lifecycle, retrieval. Durable-memory-only, no service/port/process authority of its own. | `membrane_knowledge_propose` persists with a `LifecycleReceiptV1`; `membrane_working_context` recalls across sessions. |
| **Blueprint** | Repository truth, evidence generations, drift observation. Independently usable, never independently resident — its watcher runs only inside the tray-owned daemon. | `membrane_blueprint` answers architecture/symbol/reference/impact/changes questions; unenrolled root returns typed `not_configured`, not a crash. |
| **Ledger** | Document registry, navigation, hash-bound section index. Not document truth, not durable memory. | `membrane_source_read` resolves a hash-bound `DocReadV1` reference to exact current bytes; hash mismatch fails typed. |
| **Adapt** | Mines experience into governed Taste/Insights proposals; never writes durable truth directly. | `membrane_knowledge_propose` / `membrane_feedback` surface a proposal reviewed via `adapt mine → review → adjudicate → apply → recall`; never auto-applied. |

Operational coupling (locked invariant): the visible native **tray** is the sole
resident lifecycle authority; it launches a headless **daemon** that hosts the only
Membrane runtime; MCP and CLI are stateless clients that never start or host a
runtime. Tray off ⇒ typed `membrane_unavailable { reason: "hub_inactive" }`.
Windows x86_64 is the only supported target today.

### Public surface — VERIFIED live and in source

- **MCP tools**: `engine/crates/membrane-mcp/src/tools.rs` declares **17 tools**
  total (10 `CORE` + 7 `DIAGNOSTIC`), but `tools/list` does **not** return all 17 by
  default. `negotiated_definitions()` (tools.rs:141) starts from
  `["membrane_context"]` and adds groups only when the client sends
  `params._meta["membrane.toolsets.v1"]` as an array containing `"memory"`,
  `"blueprint"`, and/or `"diagnostic"`:
  - No groups requested (or param absent): **1 tool** — `membrane_context` only.
  - `["memory"]`: **+7** (`membrane_checkpoint_save`, `membrane_checkpoint_load`,
    `membrane_working_context`, `membrane_temporal_fact`, `membrane_scratchpad`,
    `membrane_feedback`, plus `membrane_context`) = **8 tools**.
  - `["blueprint"]`: **+2** (`membrane_source_read`, `membrane_blueprint`) = **3 tools**.
  - `["diagnostic"]`: **+7** (all of `membrane_diagnostic_workspace`,
    `_mutation`, `_snapshot`, `_fence`, `_capabilities`, `_baseline`, `_provider`)
    = **8 tools**.
  - `["memory","blueprint","diagnostic"]` (all three): **17 tools** — the full
    surface only appears when a client negotiates every group.
  - Correction: prior drafts treated "17 tools" as *the* live count, and treated
    a `tools/list` response of 1 tool as a defect. Neither is right — 17 is the
    declared ceiling, 1 is the correct default-negotiation floor. **VERIFIED**
    (source read, `tools.rs:8-19,20-28,126-165`).
- **Client adapters / command-managed harnesses**: Claude Code and Codex are
  reconciled by `membrane activate`, which shells out to `claude mcp add` /
  `codex mcp add` rather than hand-editing JSON/TOML (still lands in
  `~/.claude.json` / `~/.codex/config.toml`).
- **Platform**: Windows only. A visible tray owns resident lifecycle; the daemon
  executes the runtime; the Hub dashboard is on-demand.

---

## 2. First-run checklist

Each row: check → exact command/file → exact expected value → observed 2026-09-03.

| # | Check | Exact command / file | Exact expected value | Observed |
|---|---|---|---|---|
| 1 | Hub healthy | `curl -s http://127.0.0.1:47851/health` | `ok:true`, `serviceId:"membrane-hub"`, `subsystems` = `["pull","push","cortex","blueprint","ledger","adapt"]`, `capabilities` contains `"memory"`, `protocolVersion:1`, `schemaVersion:1`, `nativeOnly:true`, `runtimeOrigin:"installed"`. | **VERIFIED PASS** — all fields match. `releaseGeneration:"sha256:unknown"` (see §5 K1, fixed-forward). `capabilities` observed as `["memory","diagnostics"]` (not just `"memory"`). |
| 2 | Tray + Hub + daemon running | `tasklist` (no filter needed — filter by `IMAGENAME eq membrane*` if you want to narrow it) | `membrane-tray.exe`, `membrane-hub.exe`-hosted `membrane-daemon.exe` running; `netstat -ano \| findstr 47851` shows `LISTENING` owned by the daemon PID. | **VERIFIED PASS** — `membrane-tray.exe` PID 158272, `membrane-daemon.exe` PID 159552 listening on 47851 (confirmed via `netstat -ano`). |
| 3 | PATH entry | New terminal (not a long-lived shell): PowerShell `[Environment]::GetEnvironmentVariable('Path','User')`, or `reg query "HKCU\Environment" /v Path` | Contains `C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current`. | **VERIFIED PASS** — confirmed present via `reg query`. **Correction**: "membrane is not on PATH" (both source docs) is **FALSE**. The failure they describe was a stale environment in a long-lived shell that started before the PATH write. **Always test PATH in a brand-new terminal.** The product's own `mcp.json` template (`…\current\mcp.json`) still uses the bare `"command":"membrane"` — this only works when PATH is current, so it is a reasonable thing to re-check per build, but it is not evidence PATH registration is broken. |
| 4 | `current` is a junction, not a symlink | `fsutil reparsepoint query "C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current"` | `Reparse Tag Value : 0xa0000003` (Mount Point / Name Surrogate), `Substitute Name` points at `versions\0.1.24`. | **VERIFIED PASS** — exact tag and target confirmed. **Correction**: prior drafts called this a "symlink." It is an NTFS junction (mount point), not a symbolic link — different reparse tag, different permission model (junctions need no elevation on Windows, symlinks by default do). |
| 5 | Activation receipt (state root) | `cat "C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\state\activation-receipt.json"` | Present; reflects last activation. | **VERIFIED PASS** — file exists at that path. |
| 6 | Idempotent re-activation | `membrane activate --install-root "C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current" --dry-run` (dry-run only — never run activate without `--dry-run` outside a deliberate change) | JSON with `runtimeOrigin:"installed"`, `service.alreadyRunning:true`, `service.state:"ready"`, every client (`codex`,`claude`,`cursor`,`windsurf`,`antigravity`) `before/after:"already_correct"`, `changed:false`. | **VERIFIED PASS** — ran live 2026-09-03; exact output matched (5 clients, all `already_correct`). `service.releaseGeneration` also read back as `"sha256:unknown"` here (same root cause as §5 K1). |
| 7 | Claude Code MCP entry | `~/.claude.json` → `mcpServers.membrane` | `type:"stdio"`, `command:"C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current\membrane.exe"`, `args:["stdio-mcp"]`. | **VERIFIED PASS** — read live, exact match. |
| 8 | Codex MCP entry | `~/.codex/config.toml` → `[mcp_servers.membrane]` | `command = 'C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current\membrane.exe'`, `args = ["stdio-mcp"]`. | **VERIFIED PASS** — read live, exact match. |
| 9 | Claude hook registration | `~/.claude/settings.json` → `hooks` keys | At minimum: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreCompact`, `PostCompact`, `PreToolUse` (matcher `.*`), `PostToolUse` (matcher `.*`), `PostToolUseFailure` (matcher `.*`), `Stop`, `TaskCompleted` — **10 Membrane-owned events**, each command `"<current>\runtime\blueprint\lib\node.exe" "<current>\mcp\hooks\membrane-hook-entrypoint.mjs"`. | **VERIFIED PASS** — counted live: `PostCompact, PostToolUse, PostToolUseFailure, PreCompact, PreToolUse, SessionEnd, SessionStart, Stop, SubagentStart, TaskCompleted, UserPromptSubmit` = 11 keys total, but `SubagentStart` and the extra `PreToolUse` entry matched to `Task\|Agent` belong to an unrelated user hook (`subagent-model-guard.py`), not Membrane. **Membrane owns exactly 10 events** (all listed above), matching `activation.rs reconcile_claude_hooks`. **Correction**: "four events" (older draft) undercounts by 6; count them from the live file, not from memory. |
| 10 | Cortex/state data present | `C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\state\` (see §4 "split data roots" — this is one of two roots) | `activation-receipt.json`, `.membrane\`, `memory-mirror\`, `tools\.cache\memory\{cortex-engine.db, cortex-engine.membrane-events.sqlite3, catalog.db, api-token, installation.json}`. | **VERIFIED PASS** — all listed files/dirs confirmed present. |
| 11 | Blueprint/Ledger data root | `%LOCALAPPDATA%\Membrane\` (this is the **other**, separate data root — see §4) | `Blueprint\` (with `state-keys\`), `context-delivery-ledger-v1\`, `ledger-index.sqlite3`. | **VERIFIED PASS** — confirmed present: `Blueprint\state-keys\<sha>.key`, `context-delivery-ledger-v1\<hash>`, `ledger-index.sqlite3` (126,976 bytes). |
| 12 | Signed build? | `membrane activate --dry-run` output, or Authenticode: `Get-AuthenticodeSignature <exe>` | **Not expected to be signed.** Signing/publishing are deferred by design until the product works. | **BY DESIGN, not a defect.** Do not list "unsigned" as a bug; note only that the qualification signature assertion (`Assert-SignedFile`) cannot pass on this build by design, and re-check when signing is turned on. |

---

## 3. Daily-use scenarios

### 3.1 Claude Code session with hooks firing
1. Open a Claude Code session (hooks are user-scope, so any directory).
2. Perform a tool action (e.g. read a file) → fires `PostToolUse` (matcher `.*`).
3. Confirm `membrane` MCP tools are callable from the session.

**Pass condition:** `~/.claude/settings.json` loads without a hook error (§2 #9);
`membrane_context` returns a packet + receipt. **UNVERIFIED live** — the hook
commands and their two referenced files exist on disk and were confirmed
registered (§2 #9), but no user-visible hook execution log was captured in this
pass; side effects surface only through context/Blueprint/Cortex events. To
observe: check Hub logs for hook/ingest lines after a tool call, or confirm a
`membrane_context` call returns non-empty `receipt` fields.

### 3.2 Context federation via `membrane_context`
1. Call `membrane_context` with `task`, `repository`, `caller{root, repositoryId, scopeId}` (all required per the JSON schema at `tools.rs:38-40`).
2. Read the returned `receipt`.

**Pass condition:** response carries a `packet` plus non-empty `receipts`, scope-bound, routed through the loopback `/federate` endpoint (never raw recall). **UNVERIFIED live** — not called in this read-only pass (the task scope is documentation-only; calling it would require a real repo/caller binding not appropriate to fabricate here).

### 3.3 Memory written and recalled across sessions
1. Call `membrane_knowledge_propose` (or `membrane_working_context` with `operation:"save"`, durable) with a fact.
2. In a new session, call `membrane_working_context`/`membrane_context` to read it back.

**Pass condition:** proposal persists with a `LifecycleReceiptV1`, and a later session's readback digest matches. **Observed baseline (VERIFIED via `/health`):** `database.status:"empty"`, `database.memoryCount:0` — no durable memory has been written yet on this machine, so the write→recall path is unproven here, not broken. Run the write→recall pair to confirm before relying on it.

### 3.4 Blueprint indexing a repository
1. With the tray/daemon running (§2 #2), call `membrane_blueprint` (`operation:"changes"` or `"snapshot_get"`) against an enrolled repo root.
2. Ask a source-identity / drift question.

**Pass condition:** `status` returns `state` in `fresh|degraded|running`; an unenrolled root returns typed `root_not_enrolled`/`not_configured`, not a crash; a pinned stale generation fails closed (`generation_mismatch`/`stale_blocked`). **UNVERIFIED live** — not exercised in this pass.

### 3.5 Push reduction on a long transcript
1. Trigger a long-transcript event (auto-compact, or manually large tool output) in a session with the hooks registered.
2. Compare delivered context size to the raw payload.

**Pass condition:** reduced view is smaller; protected spans (identifiers, exact errors, failing-test names, URLs, code fences) survive; truncation is marked (`… N lines elided …`), never silent; the raw content stays recoverable from the content-addressed artifact. **UNVERIFIED live** — Push has no dedicated MCP tool (confirmed: absent from `tools.rs` CORE/DIAGNOSTIC lists); reduction is wired into the host hook loop, not observable as a standalone call.

### 3.6 Ledger navigation
1. Obtain a hash-bound `DocReadV1` reference (from a `membrane_context` or Blueprint call).
2. Call `membrane_source_read` with `{repository, caller, sourceRef, anchorId, expectedContentHash}` (all required, `tools.rs:42-50`).

**Pass condition:** resolves to the exact indexed section; a hash/revision mismatch fails typed instead of silently returning changed text. **UNVERIFIED live** — not exercised in this pass.

### 3.7 Adapt proposals
1. `membrane adapt mine --host pi <transcript.jsonl>` → `review` → `review-taste` → `adjudicate-taste` → `apply`.
2. `membrane adapt recall "<query>" --scope workspace`.

**Pass condition:** each stage returns its typed contract (`adapt.cli.v1`, `taste_review`, 64-hex `canonical_pool_sha256`/`manifest_sha256`, `cortex_receipt.complete:true`); proposals are reviewed, never auto-applied to durable truth. **UNVERIFIED live** — not exercised in this pass.

---

## 4. Failure triage — which file answers which question

**Split data roots** — VERIFIED live 2026-09-03: Membrane's user data is split across
two separate trees on this machine, and no single doc names both:

- **Root A — `%LOCALAPPDATA%\Membrane`** (i.e. `paths.rs::data_root()`, which joins
  `PRODUCT_DIR_NAME = "Membrane"` onto the Windows LOCALAPPDATA root — source-verified
  `engine/crates/membrane-runtime/src/paths.rs:32,77-83`). Contains **Blueprint and
  Ledger** state: `Blueprint\state-keys\<sha>.key`, `context-delivery-ledger-v1\<hash>`,
  `ledger-index.sqlite3` (VERIFIED present, §2 #11).
- **Root B — `%LOCALAPPDATA%\Orthic Labs\Membrane\state`** (the installed Hub's own
  root, distinct from `paths.rs::data_root()`). Contains **Cortex, activation, and
  memory-mirror** state: `activation-receipt.json`, `memory-mirror\`, `.membrane\`,
  `tools\.cache\memory\{cortex-engine.db, cortex-engine.membrane-events.sqlite3,
  catalog.db, api-token, installation.json}` (VERIFIED present, §2 #10).

`docs/product/troubleshooting/backups.md` names only one of these two roots — check
it against both roots above before trusting "copy the data root" advice; back up
**both** trees.

| Question | File / command | Notes |
|---|---|---|
| Is the Hub healthy right now? | `curl -s http://127.0.0.1:47851/health` | Authoritative live status; VERIFIED reachable and matches contract (§2 #1). |
| Is the tray/daemon actually resident? | `tasklist` for `membrane-tray.exe`, `membrane-daemon.exe`; `netstat -ano \| findstr 47851` | VERIFIED both running, daemon PID owns the listening socket (§2 #2). |
| Was activation applied, and is it idempotent? | `membrane activate --install-root <current> --dry-run` (dry-run only) | VERIFIED — re-running is a no-op (`changed:false` for all 5 clients) (§2 #6). |
| Which client harness is registered? | `~/.claude.json` → `mcpServers.membrane`; `~/.codex/config.toml` → `[mcp_servers.membrane]`; `~/.claude/settings.json` → `hooks` | All three VERIFIED present and correct (§2 #7-9). |
| Where is Blueprint/Ledger data? | `%LOCALAPPDATA%\Membrane\` (Root A above) | VERIFIED contents (§2 #11). |
| Where is Cortex/activation/memory-mirror data? | `%LOCALAPPDATA%\Orthic Labs\Membrane\state\` (Root B above) | VERIFIED contents (§2 #10). |
| Is the build's release identity trustworthy? | `/health` → `releaseGeneration` | `"sha256:unknown"` on this build — fixed forward, see §5 K1. `serviceGeneration` (a per-process random ID, not a source-tree hash) *is* populated: `sha256:2d8ffb74…`. |
| What is installed under `current`? | `ls "…\current"` | VERIFIED: `membrane.exe`, `membrane-hub.exe`, `membrane-tray.exe`, `membrane-daemon.exe`, `cortex.exe`, `mcp.json`, `mcp\`, `runtime\`, `skills\`, `.claude-plugin\`, `.codex-plugin\`, `.antigravity-plugin\`, `LICENSE`, `release.json`. |
| Run the native doctor? | `membrane cli doctor` (not a top-level `membrane doctor` — that subcommand does not exist; **VERIFIED**: `membrane --help`/`membrane cli --help` list only `doctor, smoke, ingest, query, etc` under the `cli` passthrough, no bare `doctor`) | Needs `CORTEX_DB`/`--db`. **UNVERIFIED** whether `membrane cli doctor` (no further args) actually runs cleanly here — not executed in this pass to stay strictly read-only about state it might create. |
| No stray interpreters in the installed tree? | Search `…\current` for `node.exe`/`python*.exe` outside `runtime\blueprint\` | **UNVERIFIED** — not swept in this pass. |

---

## 5. Known-broken things — status table

| # | Item | Symptom / evidence | Status |
|---|---|---|---|
| K1 | `releaseGeneration:"sha256:unknown"` | `/health` and `activate --dry-run` both report it; root cause **VERIFIED in source**: `build.rs` (`engine/crates/membrane-runtime/build.rs:24`) emits `cargo:warning=release identity missing … MEMBRANE_SOURCE_TREE_SHA256 is unset` when `dist/release-identity.json` is absent at compile time, and falls back to `"unknown"` (`release_identity.rs`, `format!("sha256:{}", option_env!("MEMBRANE_SOURCE_TREE_SHA256").unwrap_or("unknown"))`). `apps/membrane-hub/scripts/build-frontend.mjs` **does** compute and print a real identity (`dist/release-identity.json`) as part of the normal build — this 0.1.24 build's sidecar came from a Cargo cache entry compiled *before* that file existed. Packaging now refuses to ship such a build, and `build.rs` warns at compile time. | **FIXED FORWARD** — re-verify `releaseGeneration` on the next build; if it still reads `"sha256:unknown"`, that is a new regression, not this known issue. |
| K2 | `dailyAnalysis` unavailable | `/health` → `dailyAnalysis:{"alert":true,"status":"unavailable","reason":"missing_output","lastSuccessAgeSeconds":null}` (VERIFIED live). | **Not a failed pipeline** — `missing_output` means no analysis report file has ever been written, which is indistinguishable from "never triggered." Confirm whether the trigger has ever fired before treating this as a defect. |
| K3 | Split data roots undocumented together | See §4 "Split data roots" — VERIFIED both roots exist with the contents listed. | **OPEN (doc drift)** — file this against `docs/product/troubleshooting/backups.md` / `docs/product/installation/roots.md` so a repair doesn't back up only one tree. |
| K4 | `runtime_origin()` fails open to `"installed"` | **VERIFIED in source**: `serve.rs:5025-5031`, `runtime_origin_from(None) == "installed"` (also asserted by the crate's own unit test `runtime_origin_is_explicit_and_fail_closed`, `serve.rs:5694-5699`). An unset `MEMBRANE_RUNTIME_ORIGIN` therefore claims installed origin. | **OPEN** — any dev binary run outside the `pnpm dev` wrappers without the env var set binds to the production state directory instead of a dev-scoped root. Treat as a real risk for anyone running the runtime by hand during development. |
| K5 | Unsigned build | Not signed; qualification's `Assert-SignedFile` cannot pass on it. | **BY DESIGN** — signing/publishing are deliberately deferred until the product works. Not a defect; do not re-file it as one. |
| K6 | Ten user-scope Claude hooks, matcher `.*`, run a Node process on every tool call | **VERIFIED**: §2 #9 — 10 Membrane-owned hook events, 3 of them (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`) matcher `.*`; the `Stop` hook can gate session completion. | **COST TO MEASURE, not a bug** — document the per-call latency and the fact that `Stop` can block completion; this is designed fence behavior most users won't know exists. |
| K7 | Cortex store empty | `/health` → `database.status:"empty"`, `memoryCount:0` (VERIFIED live). | **EXPECTED on a machine with no durable writes yet** — becomes a real defect only if a write is attempted and fails to persist (see §3.3). |

---

## 6. UNVERIFIED items

Not confirmed against the installed product or repo in this pass:

1. **§3.1** — user-visible evidence of hooks actually firing during a live session (registration itself is verified; execution/side-effects were not captured).
2. **§3.2** — a live `membrane_context` call and its receipt shape.
3. **§3.3** — an actual write→recall round trip through Cortex (the store is currently empty, so this is unexercised, not broken).
4. **§3.4** — a live `membrane_blueprint` call against an enrolled repo.
5. **§3.5** — a live Push reduction event and its receipt fields.
6. **§3.6** — a live `membrane_source_read` hash-bound resolution.
7. **§3.7** — a live `adapt mine → review → adjudicate → apply → recall` run.
8. **§4** — `membrane cli doctor` executed with no arguments (not run, to avoid creating state during a read-only pass).
9. **§4** — sweep of `…\current` for stray `node.exe`/`python*.exe` outside `runtime\blueprint\`.
10. Interactive installer pages and finish-page tray launch (both prior source docs flagged this as untested; not something this pass could exercise without a fresh install).
11. Clean-machine install (all evidence here is from one already-provisioned machine).
12. Whether `membrane cli doctor` (bare) fails cleanly with "no database specified" as one prior source doc claimed — plausible given the `CORTEX_DB`/`--db` requirement seen in `membrane cli --help`, but not executed live here.

---

## Report

- **File**: `D:\Claude\membrane\docs\DOGFOOD.md`
- **Sections**: 6 (What Membrane is, First-run checklist, Daily-use scenarios, Failure triage, Known-broken table, UNVERIFIED items), plus this report.
- **Claims verified live in this pass**: 19 — Hub `/health` full field set; tray/hub/daemon process residency + port ownership; PATH registry entry; junction reparse-tag type; activation receipt file presence; idempotent `activate --dry-run` output (5 clients); Claude Code MCP entry; Codex MCP entry; Claude hook event count and matchers (10 Membrane events, distinguished from 1 unrelated user hook); Root A (`%LOCALAPPDATA%\Membrane`) contents; Root B (`%LOCALAPPDATA%\Orthic Labs\Membrane\state`) contents; `membrane --version`/`--help`/`cli --help` output; `mcp.json` template using bare `"membrane"`; installed-tree file listing; `dailyAnalysis` field; Cortex `database.status`/`memoryCount`; `releaseGeneration`/`serviceGeneration` values; 17-tool declaration + negotiated-group tool counts (source-verified against `tools.rs`); `runtime_origin_from` fail-open behavior (source-verified, including the crate's own test asserting it).
- **Claims marked UNVERIFIED**: 12 (listed in §6).
- **Claims from the source documents found to be wrong**, with correction:
  1. **"membrane is not on PATH"** (both glm.md and solar.md/older draft) — **false**; PATH contains the `current` root, verified via `HKCU\Environment`. The originating failure was a stale environment in a long-lived shell, not a missing PATH entry.
  2. **"17 MCP tools" as the live `tools/list` surface, with 1 tool treated as a defect** (glm.md) — wrong on both counts: 17 is the declared ceiling across all groups; the *default* negotiated surface is 1 tool (`membrane_context`), and the full 17 requires the client to request all three groups (`memory`, `blueprint`, `diagnostic`) via `_meta["membrane.toolsets.v1"]`.
  3. **`releaseGeneration:"sha256:unknown"` framed as an unresolved medium-severity defect** (both source docs) — the root cause is known and already fixed forward (packaging now refuses builds missing `dist/release-identity.json`; `build.rs` warns at compile time). It is a "verify on next build" item, not an open bug.
  4. **`dailyAnalysis` `missing_output` framed as "the pipeline has never succeeded" / a failed pipeline** — it means no output file exists, which cannot be distinguished from "never triggered." Do not call it a failure without also checking whether the trigger has ever fired.
  5. **"Four Claude hook events registered"** (older draft of this doc) — undercounts; Membrane registers exactly 10 events (`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreCompact`, `PostCompact`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `TaskCompleted`), counted live from the file.
  6. **"`current` is a symlink"** (older draft) — it is an NTFS junction (mount point, reparse tag `0xa0000003`), confirmed via `fsutil reparsepoint query`; symlinks and junctions have different reparse tags and different Windows permission requirements.
  7. **"Unsigned build" listed as a defect** (glm.md K1/6.1.3) — by design; signing and publication are deliberately deferred until the product works. The qualification signature assertion cannot pass on this build by design, not by accident.
  8. **Single "data root" framing** (both source docs point at only `%LOCALAPPDATA%\Orthic Labs\Membrane\state`, or only `%LOCALAPPDATA%\Membrane`) — user data actually spans **two** separate trees (Root A: Blueprint/Ledger under `%LOCALAPPDATA%\Membrane`; Root B: Cortex/activation/memory-mirror under `%LOCALAPPDATA%\Orthic Labs\Membrane\state`), both verified present with distinct contents; no single prior doc named both.
