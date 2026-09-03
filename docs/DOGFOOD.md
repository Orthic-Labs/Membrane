# Membrane 0.1.24 — Daily Dogfood Checklist (Windows)

> Consolidated from the prior `docs/DOGFOOD.md` and two external reviews written
> largely without machine access, then re-verified live against the installed
> product at `%LOCALAPPDATA%\Orthic Labs\Membrane\current` (0.1.24) and the repo
> worktree on 2026-09-03. Every check states the exact command/file and the exact
> expected value. Every claim is either **VERIFIED** (checked live or in source on
> this machine) or **UNVERIFIED** (listed in §6). Claims from the source reviews
> that turned out to be wrong are corrected inline and listed again in the report.

> Third pass, 2026-09-03. Two external reviews were validated claim by claim
> against the installed product and the repo. `/health`, `activate --dry-run`, the
> Claude hook projection, the Claude/Codex MCP entries, the installed tree,
> `membrane cli doctor paths`, the runtime inventory, and the stray-interpreter
> sweep were all re-run live. One new confirmed finding was added (K8: the
> installed build writes no runtime log) and the interpreter sweep moved from
> UNVERIFIED to VERIFIED PASS. K3, K4, and K6 remain open and stay filed in
> `docs/pending/README.md`.

Paths in this document use `%LOCALAPPDATA%`, `%APPDATA%`, `%USERPROFILE%` and
`<workspace>` deliberately. Do not substitute a developer's absolute paths — this
file is tracked and must stay machine-independent.

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
| **Blueprint** | Repository truth, evidence generations, drift observation. Independently usable, never independently resident — its watcher runs only inside the tray-owned daemon. | `membrane_blueprint` answers architecture/symbol/reference/impact/changes questions; an unenrolled root returns typed `not_configured`, not a crash. Per-repo graph lives at `<repo>\.agent\graph\graph.db` (**VERIFIED** in source, `mcp/repository-catalog.mjs:146`). |
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
  default. `negotiated_definitions()` starts from `["membrane_context"]` and adds
  groups only when the client sends `params._meta["membrane.toolsets.v1"]` as an
  array containing `"memory"`, `"blueprint"`, and/or `"diagnostic"` (`"default"` is
  also accepted and adds nothing; any unknown or duplicated group makes
  `requested()` return `None`, collapsing the surface back to the floor):
  - No groups requested (or the param absent): **1 tool** — `membrane_context` only.
  - `["memory"]` → `CORE[3..]`: **+7** (`membrane_knowledge_propose`,
    `membrane_checkpoint_save`, `membrane_checkpoint_load`,
    `membrane_working_context`, `membrane_temporal_fact`, `membrane_scratchpad`,
    `membrane_feedback`) = **8 tools** with `membrane_context`.
  - `["blueprint"]` → `CORE[1..3]`: **+2** (`membrane_source_read`,
    `membrane_blueprint`) = **3 tools**.
  - `["diagnostic"]` → all 7 `DIAGNOSTIC` tools = **8 tools**.
  - `["memory","blueprint","diagnostic"]`: **17 tools** — the full surface only
    appears when a client negotiates every group.
  - Correction: prior drafts and one source review treated "17 tools" as *the* live
    count, and treated a `tools/list` response of 1 tool as a defect. Neither is
    right — 17 is the declared ceiling, 1 is the correct default-negotiation floor.
    **VERIFIED** (source read of `CORE`, `DIAGNOSTIC`, `requested()`,
    `negotiated_definitions()`).
- **Client adapters / command-managed harnesses**: Claude Code and Codex are
  reconciled by `membrane activate`, which shells out to `claude mcp add` /
  `codex mcp add` rather than hand-editing JSON/TOML (the results still land in
  `%USERPROFILE%\.claude.json` / `%USERPROFILE%\.codex\config.toml`). The literal
  path `~/.codex/config.toml` therefore appears nowhere in this repo's runtime
  code — **VERIFIED**, and immaterial: the file exists on disk with the correct
  entry (§2 #9).
- **Platform**: Windows only. A visible tray owns resident lifecycle; the daemon
  executes the runtime; the Hub dashboard is on-demand.

---

## 2. First-run checklist

Each row: check → exact command/file → exact expected value → observed 2026-09-03.
`<current>` = `%LOCALAPPDATA%\Orthic Labs\Membrane\current`.

| # | Check | Exact command / file | Exact expected value | Observed |
|---|---|---|---|---|
| 1 | Hub healthy | `curl -s http://127.0.0.1:47851/health` | `ok:true`, `serviceId:"membrane-hub"`, `subsystems` = `["pull","push","cortex","blueprint","ledger","adapt"]`, `capabilities` = `["memory","diagnostics"]`, `protocolVersion:1`, `schemaVersion:1`, `nativeOnly:true`, `runtimeOrigin:"installed"`, non-empty `installationId` and `cortexStoreId`, plus `database`, `catalog`, `planner`, `workers`, and `runtimeReceipt` blocks. | **VERIFIED PASS** — all fields match. `releaseGeneration:"sha256:unknown"` (see §5 K1, fixed forward). `catalog.status:"ok"`, `database.status:"empty"`, `database.memoryCount:0`, `planner.samples:0`, `runtimeReceipt.resolvedVersionRoot` ends in `versions\0.1.24`. |
| 2 | Port selection | `/health` on 47851 | 47851 is the installed default (`engine/crates/membrane-runtime/src/service.rs`, `port: 47_851`). A `<workspace>\tools\lib\memory\runtime.json` with `schemaVersion:1`, `serviceId:"membrane-local-v1"`, `host:"127.0.0.1"`, `port >= 1024` overrides it. | **VERIFIED PASS** — 47851 live; the override path is `tools/lib/memory/runtime.json` (source-verified, `service.rs:284,448`). **Correction**: one source review said the port comes from an installed `runtime/runtime.json`. No such file exists — `<current>\runtime\` contains only `blueprint\`, `resources\`, `runtime-inventory.json`. |
| 3 | Tray + daemon running | `tasklist` (narrow with `IMAGENAME eq membrane*` if you like) | `membrane-tray.exe` and `membrane-daemon.exe` both present; `netstat -ano` filtered on 47851 shows `LISTENING` owned by the daemon PID. | **VERIFIED PASS** — both resident; the daemon owns the listening socket. Expect additional short-lived `membrane.exe` processes, one per `stdio-mcp` client session; these are stateless clients, not extra runtimes. |
| 4 | PATH entry | **New** terminal (never a long-lived shell): `[Environment]::GetEnvironmentVariable('Path','User')`, or `reg query "HKCU\Environment" /v Path` | Contains `%LOCALAPPDATA%\Orthic Labs\Membrane\current`. | **VERIFIED PASS** (earlier pass, via `reg query`). **Correction**: "membrane is not on PATH" (both source reviews) is **FALSE**. The failure they describe was a stale environment in a long-lived shell that started before the PATH write. Always test PATH in a brand-new terminal. `<current>\mcp.json` still uses the bare `"command":"membrane"`, which only resolves when PATH is current — worth re-checking per build, but not evidence PATH registration is broken. |
| 5 | `current` is a junction, not a symlink | `fsutil reparsepoint query "<current>"` | `Reparse Tag Value : 0xa0000003` (Mount Point / Name Surrogate), `Substitute Name` pointing at `versions\0.1.24`. | **VERIFIED PASS** — exact tag and target confirmed; `/health` `runtimeReceipt` independently reports `resolvedVersionRoot` = `…\versions\0.1.24` and `stableInstallRoot` = `…\current`. **Correction**: prior drafts called this a "symlink." It is an NTFS junction (mount point) — different reparse tag, different permission model (junctions need no elevation; symlinks by default do). |
| 6 | Activation receipt | `%LOCALAPPDATA%\Orthic Labs\Membrane\state\activation-receipt.json` | Present; reflects the last activation. | **VERIFIED PASS** — file exists at that path. |
| 7 | Idempotent re-activation | `membrane activate --install-root "<current>" --dry-run` (dry-run only — never run `activate` without `--dry-run` outside a deliberate change) | JSON with `runtimeOrigin:"installed"`, `service.port:47851`, `service.alreadyRunning:true`, `service.state:"ready"`, and every client (`codex`, `claude`, `cursor`, `windsurf`, `antigravity`) `before/after:"already_correct"`, `changed:false`. | **VERIFIED PASS** — re-run live 2026-09-03; exact output matched, all 5 clients `already_correct`, `changed:false`. `service.releaseGeneration` also reads `"sha256:unknown"` (same root cause as §5 K1). |
| 8 | Claude Code MCP entry | `%USERPROFILE%\.claude.json` → `mcpServers.membrane` | `type:"stdio"`, `command:"<current>\membrane.exe"`, `args:["stdio-mcp"]`. | **VERIFIED PASS** — read live; exact match (`env` is `{}`). |
| 9 | Codex MCP entry | `%USERPROFILE%\.codex\config.toml` → `[mcp_servers.membrane]` | `command` = `<current>\membrane.exe`, `args = ["stdio-mcp"]`. | **VERIFIED PASS** — read live; exact match. |
| 10 | Claude hook registration | `%USERPROFILE%\.claude\settings.json` → `hooks` | Exactly **10 Membrane-owned events**: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreCompact`, `PostCompact`, `PreToolUse` (matcher `.*`), `PostToolUse` (matcher `.*`), `PostToolUseFailure` (matcher `.*`), `Stop`, `TaskCompleted` — each command `"<current>\runtime\blueprint\lib\node.exe" "<current>\mcp\hooks\membrane-hook-entrypoint.mjs"`. | **VERIFIED PASS** — counted live: all 10 present, 3 with matcher `.*`, matching `activation.rs reconcile_claude_hooks`. `PreToolUse` also carries one **non-Membrane** group (an unrelated user hook matched to `Task`/`Agent`) — count Membrane-owned groups, not raw keys. **Correction**: "four events" (older draft) undercounts by 6; count them from the live file, not from memory. |
| 11 | Cortex / activation state root | `%LOCALAPPDATA%\Orthic Labs\Membrane\state\` (Root B — see §4 "Split data roots") | `activation-receipt.json`, `.membrane\`, `memory-mirror\`, `tools\.cache\memory\{cortex-engine.db, cortex-engine.membrane-events.sqlite3, catalog.db, api-token, installation.json}`. | **VERIFIED PASS** — all present, and `/health` `runtimeReceipt` names this tree as `workspaceRoot`, with `cortexDb`, `catalogDb`, and `telemetryEventDb` all under it. |
| 12 | Blueprint / Ledger data root | `%LOCALAPPDATA%\Membrane\` (Root A — see §4) | `Blueprint\` (with `state-keys\`), `context-delivery-ledger-v1\`, `ledger-index.sqlite3`. | **VERIFIED PASS** — confirmed present; `ledger-index.sqlite3` is non-empty and recently written. |
| 13 | Declared roots vs. real roots | `membrane cli doctor paths` | JSON `{"schemaVersion":1,"product":"Membrane","roots":{…},"receiptOwned":[…]}` with `config` = `%APPDATA%\Membrane`, and `data` = `cache` = `log` = `%LOCALAPPDATA%\Membrane`. | **VERIFIED PASS on the contract, but the declared roots are not the whole truth.** Exact output matched, `receiptOwned` empty. `%APPDATA%\Membrane` holds only `first-run.json`; the declared `data`/`log` root holds Blueprint/Ledger state and **no log file at all**; Cortex lives under Root B, which `doctor paths` never names. See §5 K3 and K8. |
| 14 | Runtime inventory | `<current>\runtime\runtime-inventory.json` | `schemaVersion:3`, `app:"membrane-hub"`, `target:"x86_64-pc-windows-msvc"`, one `axes` entry per subsystem, non-empty `entries` including the `blueprint-runtime` component binding `runtime\blueprint\lib\node.exe` by SHA-256. | **VERIFIED PASS** — `schemaVersion:3`, correct target, six axes (`pull`, `push`, `cortex`, `blueprint`, `ledger`, `adapt`); `composition` lists all seven names. |
| 15 | Installed tree contents | `ls -a "<current>"` | Sidecars `membrane.exe`, `membrane-hub.exe`, `membrane-tray.exe`, `membrane-daemon.exe`, `cortex.exe`; `mcp\`, `runtime\`, `skills\`; `mcp.json`, `plugin.json`, `release.json`, `rightax-portable-core.json`, `LICENSE`, `THIRD_PARTY_NOTICES.md`; hidden `.agents\`, `.claude-plugin\`, `.codex-plugin\`, `.antigravity-plugin\`. | **VERIFIED PASS** — exact listing confirmed. Use `-a`: four entries are dot-prefixed and a plain `ls` hides them. `release.json` carries `version:"0.1.24"` and the same `releaseGeneration:"sha256:unknown"`. |
| 16 | No stray interpreters in the installed tree | Search `<current>` for `node.exe` / `python*.exe` outside `runtime\blueprint\` | Only the inventory-bound `runtime\blueprint\lib\node.exe`; qualification fails on any other interpreter executable. | **VERIFIED PASS** — swept live: exactly one `node.exe` (the inventory-bound one), zero Python executables. Three `.py` files ship as Blueprint eval **fixture data** under `runtime\blueprint\app\package\evals\fixture-repos\python-context\`; they are inert test inputs, not an interpreter. Re-sweep every build — `adapt/README.md:118` records that exact package exclusion is still tracked by the native migration ledger. |
| 17 | Signed build? | `Get-AuthenticodeSignature <exe>` | **Not expected to be signed.** Signing and publishing are deferred by deliberate decision until the product works. | **BY DESIGN, not a defect.** Do not list "unsigned" as a bug. Note only that the qualification signature assertion (`Assert-SignedFile`, requiring Authenticode `Valid` plus a trusted timestamp) cannot pass on this build by design; re-check when signing is turned on. |

---

## 3. Daily-use scenarios

### 3.1 Claude Code session with hooks firing
1. Open a Claude Code session (hooks are user-scope, so any directory).
2. Perform a tool action (e.g. read a file) → fires `PostToolUse` (matcher `.*`).
3. Confirm `membrane` MCP tools are callable from the session.

**Pass condition:** `%USERPROFILE%\.claude\settings.json` loads without a hook error
(§2 #10); hook envelopes come back with `hookSpecificOutput.hookEventName` matching
the event and `membraneHook.results[].id` values drawn from the module ids in
`mcp/hooks/membrane-hook-runtime.mjs`; a blocked `PreToolUse`/`Stop` returns
`"decision":"block"` with a reason. **UNVERIFIED live** — registration and both
referenced files are confirmed on disk, but no hook execution output was captured.
Observing this is harder than it should be: the installed build writes no runtime
log (§5 K8), so the only current evidence path is a `membrane_context` call
returning non-empty receipt fields.

### 3.2 Context federation via `membrane_context`
1. Call `membrane_context` with `task`, `repository`, and `caller{root, repositoryId, scopeId}` (all required per the tool's JSON schema).
2. Read the returned `receipt`.

**Pass condition:** the response carries a `packet` plus non-empty `receipts`,
scope-bound, routed through the loopback `/federate` endpoint (never raw recall).
**UNVERIFIED live** — not called in this read-only pass; a real repo/caller binding
would have to be fabricated to force it.

### 3.3 Memory written and recalled across sessions
1. Call `membrane_knowledge_propose` (or `membrane_working_context` with `operation:"save"`, `durable:true`, authority `A1` plus source refs) with a fact.
2. In a new session, call `membrane_working_context` with `operation:"load"` and the same `sessionId`/`taskId`.

**Pass condition:** the proposal returns `status:"needs_review"` with a
`membrane.lifecycle-receipt.v1` carrying `durable_id`, `event_id`, and
`readback_digest`; a later session's readback digest matches. A readback mismatch
must be a hard tool error, never a silent pass. **Observed baseline (VERIFIED via
`/health`):** `database.status:"empty"`, `database.memoryCount:0`,
`catalog.receipts:0`, `catalog.activeGrants:0` — no durable memory has been written
on this machine, so the write→recall path is **unproven here, not broken**. Run the
write→recall pair before relying on it.

### 3.4 Blueprint indexing a repository
1. With the tray/daemon running (§2 #3), call `membrane_blueprint` (`operation:"changes"` or `"snapshot_get"`) against an enrolled repo root.
2. Ask a source-identity / drift question.

**Pass condition:** `status` returns `state` in `fresh|degraded|running`; an
unenrolled root returns typed `root_not_enrolled`/`not_configured`/`graph_missing`,
not a crash; a generation pinned to a bogus `sha256:000…0` fails closed with
`generation_mismatch` or `stale_blocked`, never stale data. With the tray off, the
bounded one-shot path is `<current>\runtime\blueprint\bin\blueprint.cmd status
--json --root <repo>` (**VERIFIED present**, alongside `blueprint-mcp.cmd`); it must
never start a watcher or daemon. **UNVERIFIED live** — no Blueprint query was issued
in this pass.

### 3.5 Push reduction on a long transcript
1. Trigger a long-transcript event (auto-compact, or a manually large tool output) in a session with the hooks registered.
2. Compare delivered context size to the raw payload.

**Pass condition:** the reduced view is smaller; protected spans (identifiers, exact
errors, failing-test names, URLs, code fences) survive; truncation is marked
(`… N lines elided …`), never silent; raw content stays recoverable from the
content-addressed artifact. **UNVERIFIED live** — Push has no dedicated MCP tool
(confirmed absent from the `CORE`/`DIAGNOSTIC` lists); reduction is wired into the
host hook loop, not observable as a standalone call. Repo note, **VERIFIED**: the
shipped `runc` path is shell-backed (`engine/crates/membrane-runtime/src/push/runc.rs`
spawns via `Command::new`), and allow-listed adapter wiring is recorded as partial
in the Push canon rows already indexed in `docs/pending/README.md`. That is tracked
capability work, not a new dogfood defect.

### 3.6 Ledger navigation
1. Obtain a hash-bound `DocReadV1` reference (from a `membrane_context` or Blueprint call).
2. Call `membrane_source_read` with `{repository, caller, sourceRef, anchorId, expectedContentHash}` (all required).

**Pass condition:** resolves to the exact indexed section; a hash/revision mismatch
fails typed instead of silently returning changed text. **UNVERIFIED live** — not
exercised. `ledger-index.sqlite3` exists and is being written (§2 #12), so the index
is live even though resolution was not tested.

### 3.7 Adapt proposals
1. `membrane adapt mine --host pi <transcript.jsonl>` → `review` → `review-taste` → `adjudicate-taste` → `apply` (needs `CORTEX_DB` plus `MEMBRANE_WORKSPACE_ROOT`, or `--db`).
2. `membrane adapt recall "<query>" --scope workspace`.

**Pass condition:** each stage returns its typed contract
(`response.api_version:"adapt.cli.v1"`, a `taste_review` binding
`adapt.taste-review-input.v1`, 64-hex `canonical_pool_sha256` and `manifest_sha256`,
`apply` → `response.valid:true` with `cortex_receipt.complete:true`, `recall` →
admitted records with a lifecycle state); proposals are reviewed, never auto-applied
to durable truth. **UNVERIFIED live** — not exercised in this pass.

---

## 4. Failure triage — which file answers which question

**Split data roots** — VERIFIED live 2026-09-03. Membrane's user state is split
across **three** trees, and `membrane cli doctor paths` names only two of them:

- **Root A — `%LOCALAPPDATA%\Membrane`** (`paths.rs::data_root()`, which joins
  `PRODUCT_DIR_NAME = "Membrane"` onto the Windows LOCALAPPDATA root). Declared by
  `doctor paths` as `data`, `cache`, **and** `log`. Actually contains **Blueprint and
  Ledger** state only: `Blueprint\state-keys\<sha>.key`,
  `context-delivery-ledger-v1\<hash>`, `ledger-index.sqlite3`. Contains **no log
  file** — see §5 K8.
- **Root B — `%LOCALAPPDATA%\Orthic Labs\Membrane\state`** (the installed Hub's own
  workspace root, which `doctor paths` never names). Contains **Cortex, activation,
  and memory-mirror** state: `activation-receipt.json`, `memory-mirror\`,
  `.membrane\`, `tools\.cache\memory\{cortex-engine.db,
  cortex-engine.membrane-events.sqlite3, catalog.db, api-token, installation.json}`.
  `/health` `runtimeReceipt.workspaceRoot` points here.
- **Root C — `%APPDATA%\Membrane`** — declared by `doctor paths` as `config`;
  currently holds only `first-run.json`.

`docs/product/troubleshooting/backups.md` names only one of these — check it against
all three before trusting "copy the data root" advice. Back up **both** Root A and
Root B; Cortex and Blueprint/Ledger do not live in the same tree.

| Question | File / command | Notes |
|---|---|---|
| Is the Hub healthy right now? | `curl -s http://127.0.0.1:47851/health` | Authoritative live status; VERIFIED reachable and matching contract (§2 #1). |
| Is the tray/daemon actually resident? | `tasklist` for `membrane-tray.exe` and `membrane-daemon.exe`; `netstat -ano` filtered on 47851 | VERIFIED both running, daemon PID owns the listening socket (§2 #3). Extra `membrane.exe` processes are per-session stdio-MCP clients, not runtimes. |
| Was activation applied, and is it idempotent? | `membrane activate --install-root "<current>" --dry-run` (dry-run only) | VERIFIED — re-running is a no-op (`changed:false` for all 5 clients) (§2 #7). |
| Which client harness is registered? | `%USERPROFILE%\.claude.json` → `mcpServers.membrane`; `%USERPROFILE%\.codex\config.toml` → `[mcp_servers.membrane]`; `%USERPROFILE%\.claude\settings.json` → `hooks` | All three VERIFIED present and correct (§2 #8–10). |
| Did the installer fail, and at which step? | `%LOCALAPPDATA%\Orthic Labs\Membrane\logs\install-<version>.log` | VERIFIED present for 0.1.24. NSIS writes step-level `<step> exit=<code>` lines. This is currently the **only** log file on the machine. |
| Where are the Hub's runtime logs? | Nowhere on this build — see §5 K8. | The declared log root (`%LOCALAPPDATA%\Membrane`) contains no log. The repo fix appends Hub stdout/stderr to `<log root>\membrane-hub.log`; verify it on the next installed build. |
| Where is Blueprint/Ledger data? | `%LOCALAPPDATA%\Membrane\` (Root A) | VERIFIED contents (§2 #12). |
| Where is Cortex/activation/memory-mirror data? | `%LOCALAPPDATA%\Orthic Labs\Membrane\state\` (Root B) | VERIFIED contents (§2 #11). |
| What roots does the product *claim*? | `membrane cli doctor paths` | VERIFIED output, but incomplete — it never names Root B, where Cortex actually lives (§5 K3). |
| Is the build's release identity trustworthy? | `/health` → `releaseGeneration`; also `<current>\release.json` | `"sha256:unknown"` on this build — fixed forward, see §5 K1. `serviceGeneration` (a per-process random id, not a source-tree hash) *is* populated. Qualification's `Normalize-Generation` strips a `sha256:` prefix then requires `^[0-9a-f]{64}$`, so it rejects this value outright. |
| What is installed under `current`? | `ls -a "<current>"` | VERIFIED listing (§2 #15) — use `-a`, four of the entries are dot-prefixed. |
| What exactly shipped, with hashes? | `<current>\runtime\runtime-inventory.json`; `%LOCALAPPDATA%\Orthic Labs\Membrane\{checksums.json, release-manifest.json, sbom-windows-x86_64.cdx.json, provenance-windows-x86_64.intoto.jsonl}` | VERIFIED present. The inventory lists every file with SHA-256; repair proves the tree matches it. |
| Is the Blueprint pipe healthy? | Named pipe `\\.\pipe\membrane-blueprint-<first 16 hex of sha256(USERPROFILE)>` (derivation in `scripts/qualification/install-release.ps1`, `Get-BlueprintEndpoint`) | Contract source-verified; **UNVERIFIED live** — pipe enumeration was not run in this pass. |
| Is Blueprint reachable with the tray off? | `<current>\runtime\blueprint\bin\blueprint.cmd status --json --root <repo>` | File VERIFIED present. Bounded one-shot; exit 0 or 2 with typed missing/`not_configured` states; must never start a watcher. |
| Run the native doctor? | `membrane cli doctor --bundle ./membrane-diagnostic.json` (content-free support bundle). There is **no** top-level `membrane doctor` — **VERIFIED**: `membrane --help` and `membrane cli --help` expose `doctor` only under the `cli` passthrough. | Needs `CORTEX_DB`/`--db`. **UNVERIFIED** whether bare `membrane cli doctor` runs cleanly here — not executed, to avoid creating state during a read-only pass. |
| Data-root repairs | Read `docs/product/troubleshooting/backups.md` first — then apply the Root A/Root B correction above. | Copy both roots before any repair; never delete or compact a live root to clear a Hub alert. |

---

## 5. Known-broken things — status table

| # | Item | Symptom / evidence | Status |
|---|---|---|---|
| K1 | `releaseGeneration:"sha256:unknown"` | `/health`, `activate --dry-run`, and `<current>\release.json` all report it (VERIFIED live). Root cause **VERIFIED in source**: `engine/crates/membrane-runtime/build.rs` emits a `release identity missing … MEMBRANE_SOURCE_TREE_SHA256 is unset` compile warning when `dist/release-identity.json` is absent, and `release_identity.rs` falls back to `format!("sha256:{}", option_env!("MEMBRANE_SOURCE_TREE_SHA256").unwrap_or("unknown"))`. The frontend build script **does** compute a real identity during a normal build; this 0.1.24 sidecar came from a Cargo cache entry compiled *before* `dist/release-identity.json` existed. Packaging now refuses to ship such a build and `build.rs` warns at compile time. | **FIXED FORWARD** — re-verify `releaseGeneration` on the next build. If it still reads `"sha256:unknown"`, that is a new regression, not this known issue. Do not re-file it as an open defect. |
| K2 | `dailyAnalysis` unavailable | `/health` → `dailyAnalysis:{"alert":true,"status":"unavailable","reason":"missing_output","lastSuccessAgeSeconds":null}` (VERIFIED live). | **Not a failed pipeline** — `missing_output` means no analysis report file has ever been written, which is indistinguishable from "never triggered." Confirm whether the trigger has ever fired before treating this as a defect. |
| K3 | Declared roots do not match real roots | §4 "Split data roots" — VERIFIED live. `membrane cli doctor paths` names `%APPDATA%\Membrane` and `%LOCALAPPDATA%\Membrane`, but the Hub's own `/health` `runtimeReceipt.workspaceRoot` is `%LOCALAPPDATA%\Orthic Labs\Membrane\state`, and every Cortex/catalog/event DB lives there. | **OPEN (doc drift)** — filed in `docs/pending/README.md`. Triage-critical: following `backups.md` today copies the wrong tree. |
| K4 | `runtime_origin()` fails open to `"installed"` | **VERIFIED in source**: `serve.rs` `runtime_origin_from(None) == "installed"`, also asserted by the crate's own unit test. An unset `MEMBRANE_RUNTIME_ORIGIN` therefore claims installed origin. | **OPEN** — filed in `docs/pending/README.md`. Any dev binary run outside the `pnpm dev` wrappers without the env var set binds to the production state directory instead of a dev-scoped root. |
| K5 | Unsigned build | Not signed; qualification's `Assert-SignedFile` (Authenticode `Valid` plus a trusted timestamp) cannot pass on it. | **BY DESIGN** — signing and publishing are deliberately deferred until the product works. Not a defect; do not re-file it as one. |
| K6 | Ten user-scope Claude hooks, three with matcher `.*`, run a Node process on every tool call | **VERIFIED live**: §2 #10 — 10 Membrane-owned hook events, 3 (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`) with matcher `.*`; the `Stop` hook can gate session completion. | **COST TO MEASURE, not a bug** — filed in `docs/pending/README.md`. Document the per-call latency and the fact that `Stop` can block completion; this is designed fence behavior most users will not know exists. |
| K7 | Cortex store empty | `/health` → `database.status:"empty"`, `memoryCount:0`, `catalog.receipts:0`, `catalog.activeGrants:0` (VERIFIED live). | **EXPECTED on a machine with no durable writes yet** — becomes a real defect only if a write is attempted and fails to persist (see §3.3). |
| K8 | The installed build writes no runtime log | **VERIFIED live**: `%LOCALAPPDATA%\Orthic Labs\Membrane\logs\` contains exactly one file, `install-0.1.24.log`; the log root `doctor paths` declares (`%LOCALAPPDATA%\Membrane`) contains no log file at all. The tray launched the Hub with stdout and stderr discarded, so a live problem left nothing to read. | **FIXED IN REPO, PENDING VERIFICATION ON THE INSTALLED BUILD.** The committed fix appends both Hub streams to `<log root>\membrane-hub.log` — the same root `membrane cli doctor paths` reports, or `MEMBRANE_LOG_ROOT` when set — and falls back to the old behavior if logging fails, so it can never block startup. That fix is **not** in 0.1.24. On the next installed build, confirm `%LOCALAPPDATA%\Membrane\membrane-hub.log` exists and grows. Until then, §3.1's "check the logs" advice has no log to check. |
| K9 | Diagnostics worker overload rejections observed | `/health` → `workers.overloadRejections.diagnostics` non-zero with `workers.diagnostics.max:1` (VERIFIED live). | **UNVERIFIED significance** — a single diagnostics worker rejecting concurrent requests may be correct backpressure or a real bottleneck. Not filed as a defect; capture the count on a fresh build before drawing a conclusion. |

---

## 6. UNVERIFIED items

Not confirmed against the installed product or the repo in this pass:

1. **§3.1** — user-visible evidence of hooks actually firing during a live session (registration is verified; execution and side effects were not captured, and K8 means there is no runtime log to read).
2. **§3.2** — a live `membrane_context` call and its receipt shape.
3. **§3.3** — an actual write→recall round trip through Cortex (the store is empty, so this is unexercised, not broken).
4. **§3.4** — a live `membrane_blueprint` call against an enrolled repo, and the fail-closed `generation_mismatch`/`stale_blocked` behavior.
5. **§3.5** — a live Push reduction event and its receipt fields.
6. **§3.6** — a live `membrane_source_read` hash-bound resolution.
7. **§3.7** — a live `adapt mine → review → adjudicate → apply → recall` run.
8. **§4** — bare `membrane cli doctor` executed (not run, to avoid creating state during a read-only pass), including whether it fails cleanly with a "no database specified" error as one source review implied.
9. **§4** — the Blueprint named pipe was not enumerated; only its derivation rule is source-verified.
10. **§4** — the tray-off bounded one-shot `blueprint.cmd status --json --root <repo>` was not executed; only its presence on disk is verified.
11. **§5 K9** — whether the observed diagnostics overload rejections indicate a real bottleneck.
12. Tray icon and popover behavior, and the single-instance dashboard cutover (two or more `Membrane Hub` windows, second process exit code 0) — process residency is verified, the window/UI assertions are not.
13. Interactive installer pages and finish-page tray launch (both source reviews flagged this as untested; not exercisable without a fresh install).
14. Clean-machine install (all evidence here comes from one already-provisioned machine).

Items 13 and 14 are the largest coverage gap in the product: activation is the
single transaction that writes the Claude hooks, the MCP bindings, and PATH, and
every §2 check flows through it — yet the interactive path most users will take has
never been run end to end. That is a real gap, but it is unverifiable from this
machine rather than a confirmed defect, so it is recorded here and not filed as
pending work.

---

## Report

- **File**: `docs/DOGFOOD.md`
- **Sections**: 6 (What Membrane is, First-run checklist, Daily-use scenarios, Failure triage, Known-broken table, UNVERIFIED items), plus this report.
- **Claims verified in this pass**: Hub `/health` full field set including `catalog`, `planner`, `workers`, and `runtimeReceipt`; port 47851 and the `tools/lib/memory/runtime.json` override rule; tray/daemon residency and port ownership; junction reparse target; activation receipt presence; idempotent `activate --dry-run` (5 clients, `changed:false`); the Claude MCP entry; the Codex MCP entry; the 10 Membrane-owned Claude hook events and their matchers, distinguished from 1 unrelated user hook; Root A, Root B, and Root C contents; `membrane cli doctor paths` output; runtime inventory `schemaVersion`, target, and axes; the full installed tree including dot-prefixed plugin directories; the stray-interpreter sweep (one inventory-bound `node.exe`, zero Python executables); `release.json` version and generation; the MCP tool-negotiation table read from `CORE`, `DIAGNOSTIC`, `requested()`, and `negotiated_definitions()`; `Normalize-Generation` in the qualification script; `runtime_origin_from` fail-open; the shell-backed Push `runc` path; `blueprint.cmd` presence; and the absence of any runtime log.
- **Claims marked UNVERIFIED**: 14 (listed in §6).
- **Claims from the source reviews found to be wrong**, with correction:
  1. **"membrane is not on PATH"** (both reviews) — **false**; PATH contains the `current` root. The originating failure was a stale environment in a long-lived shell.
  2. **"17 MCP tools" as the live `tools/list` surface, with 1 tool treated as a defect** — wrong on both counts: 17 is the declared ceiling across all groups; the default negotiated surface is 1 tool, and 17 requires the client to request all three groups.
  3. **`releaseGeneration:"sha256:unknown"` framed as an unresolved open defect** (both reviews) — root-caused and fixed forward; it is a "verify on the next build" item.
  4. **`releaseGeneration` should be a bare 64-hex string, not a `sha256:` wrapper** (one review) — wrong. The runtime contract emits `sha256:<64-hex>`; `Normalize-Generation` strips the prefix before validating. The prefix is correct; only `unknown` is wrong.
  5. **The Hub port comes from an installed `runtime/runtime.json`** (one review) — wrong. No such file exists under `<current>\runtime\`; the override is `<workspace>\tools\lib\memory\runtime.json`.
  6. **`dailyAnalysis` `missing_output` framed as a failed pipeline** — it means no output file exists, which cannot be distinguished from "never triggered."
  7. **"Four Claude hook events registered"** (older draft) — undercounts; Membrane registers exactly 10.
  8. **"`current` is a symlink"** (older draft) — it is an NTFS junction (reparse tag `0xa0000003`), a different tag and a different permission model.
  9. **"Unsigned build" listed as a defect** (one review) — signing and publication are deliberately deferred. The qualification signature assertion cannot pass by design, not by accident.
  10. **A single "data root"** (both reviews) — state actually spans three trees, and the two that matter for backup (Root A and Root B) sit in different parent directories.
  11. **"A signed tray executable" as the expected first-run state** (one review) — same as (9); not expected on this build.

## Open findings, 2026-09-03

Confirmed on the installed 0.1.24 build while validating two external reviews.
They live here rather than in `docs/pending/`, whose contents are a frozen
capability inventory that `scripts/ci/check-atomic-canons.mjs` owns.

## Dogfood audit findings (2026-09-03)

Findings confirmed live against the installed product and repo while validating
two external reviews against
`docs/DOGFOOD.md`. Filed here because they are still open after validation;
findings that were already fixed, wrong, or unverifiable were not filed (see
`docs/DOGFOOD.md` §5–6 for the full disposition).

| Finding | Evidence | Fix required |
|---|---|---|
| Membrane user data is split across two undocumented roots: `%LOCALAPPDATA%\Membrane` (Blueprint/Ledger state) and `%LOCALAPPDATA%\Orthic Labs\Membrane\state` (Cortex/activation/memory-mirror state). No single doc names both. | Confirmed live 2026-09-03: both roots present with the contents listed in `docs/DOGFOOD.md` §2 rows 10–11 and §4 "Split data roots". `docs/product/troubleshooting/backups.md` names only one root. | Update `docs/product/troubleshooting/backups.md` and `docs/product/installation/roots.md` to name both roots explicitly, so a repair backs up both trees, not just one. |
| `runtime_origin()` fails open to `"installed"` when `MEMBRANE_RUNTIME_ORIGIN` is unset. | Source-verified in `engine/crates/membrane-runtime/src/serve.rs` (`runtime_origin_from(None) == "installed"`), including the crate's own unit test asserting this behavior. | A dev binary run outside the `pnpm dev` wrappers without the env var set silently binds to the production state directory instead of a dev-scoped root. Needs a fail-closed default (or an explicit warning) so accidental dev runs cannot write into installed-origin state. |
| Ten user-scope Claude Code hooks are registered with `matcher: ".*"` (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`), running a Node process on every matched tool call in every session; the `Stop` hook can gate session completion. | Confirmed live 2026-09-03: `%USERPROFILE%\.claude\settings.json` carries exactly 10 Membrane-owned hook events matching `activation.rs reconcile_claude_hooks`, 3 with matcher `.*`. This is designed fence behavior, not a bug. | Measure and document the per-call latency cost, and document that `Stop` can deny session completion — most users are not aware this gate exists. |

## Dogfood audit findings (2026-09-03, second validation pass)

Two external reviews of Membrane 0.1.24 were validated claim by claim against the
installed product and the repo. Only findings that were CONFIRMED and are still
open are filed below; findings that were already fixed, wrong, or unverifiable were
not filed. Full disposition, including every rejected claim, is in
`docs/DOGFOOD.md` §5–6. The findings filed in the first pass above
(split data roots, `runtime_origin()` fail-open, ten `.*`-matched Claude hooks)
remain open and are not restated here.

| Finding | Evidence | Fix required |
|---|---|---|
| The installed 0.1.24 build writes no runtime log at all, so a live Hub problem leaves nothing to read. | Confirmed live 2026-09-03: `%LOCALAPPDATA%\Orthic Labs\Membrane\logs\` contains exactly one file, `install-0.1.24.log`, and the log root `membrane cli doctor paths` declares (`%LOCALAPPDATA%\Membrane`) contains no log file. The tray launched the Hub with stdout and stderr discarded. | Fixed in the repo — the tray now appends both Hub streams to `<log root>\membrane-hub.log`, falling back to the old behavior if logging fails. **Pending verification**: that fix is not in the installed 0.1.24 build. On the next installed build, confirm `%LOCALAPPDATA%\Membrane\membrane-hub.log` is created and grows; only then is this closed. |
| `membrane cli doctor paths` declares a `log` root that nothing writes to, which sends triage to an empty directory. | Confirmed live 2026-09-03: `doctor paths` reports `log` = `%LOCALAPPDATA%\Membrane`; that tree holds only Blueprint/Ledger state (`Blueprint\state-keys\`, `context-delivery-ledger-v1\`, `ledger-index.sqlite3`) and no log. The only log on the machine is the NSIS installer log under `%LOCALAPPDATA%\Orthic Labs\Membrane\logs\`. | Either make the declared `log` root the root that is actually written (the committed tray fix does this), or have `doctor paths` report the roots the running Hub actually uses. The two must agree, because `doctor paths` is the documented triage entry point. Verify together with the row above on the next installed build. |
