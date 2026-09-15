# Membrane plugin & MCP — target shape

**Status:** accepted architecture target; implementation & qualification remain separate  
**Primary authority:** `docs/architecture/adr/2026-09-12-context-system-decisions.md`  
**Scope:** installed plugin discovery, MCP transport, SessionStart activation, shared-engine ownership, authorization, Pull, public `push`, repair, rollback & installed-host proof

## 1. Product boundary

```text
Membrane installer / updater
  -> verifies complete versioned payload
  -> switches stable current
  -> installs or repairs supported host plugin projections
  -> provisions host-scoped MCP authentication
  -> verifies plugin discovery, MCP binding & exact installed identity

Codex / Claude plugin
  -> exposes Membrane skill, metadata & native SessionStart hook
  -> declares one authenticated Streamable-HTTP MCP connection
  -> SessionStart starts or adopts shared installed engine & acquires harness ownership

stdio-only host projection
  -> launches stable membrane-client stdio-mcp
  -> client starts or adopts same installed engine, forwards MCP & holds harness ownership

shared Membrane engine
  -> owns one planner, embedder, stores, watchers & subsystem runtime per OS user
  -> serves Pull, public push, resources, prompts, receipts & status
  -> drains & stops after final Hub/harness owner releases
```

Plugin supplies host integration. MCP supplies callable operations. SessionStart supplies bounded orientation. Membrane planner alone chooses context evidence.

## 2. Separate observable states

Never collapse these states into `connected` or `activated`:

1. Release payload installed & stable `current` verified.
2. Host detected.
3. Plugin projection installed.
4. Plugin enabled.
5. SessionStart hook trusted & observed.
6. One intended MCP binding installed.
7. MCP transport initialized & tools/resources listed.
8. Harness ownership acquired against exact installed engine.
9. Repository/scope/caller grant valid.
10. Selected operation available.
11. Pull or public `push` completed with typed receipt.

Success at one state never proves any later state. Authorization rejection proves transport reachability only.

## 3. Ownership

| Owner | Owns | Does not own |
|---|---|---|
| Installer/bootstrap | Candidate verification, versioned payload, stable `current`, rollback, repair, uninstall | Runtime planning or user grants |
| `membrane activate` | Host discovery, plugin projection, MCP authentication/config, hooks, exact reconciliation & activation receipt | Host features absent from that host |
| Portable plugin | Canonical identity, version, public skill, portable MCP description, OpenAI metadata & root-relative assets | Engine lifetime or duplicate global MCP registration |
| Native host adapter | Host-specific install/enablement, trust state, event schema, credentials & cleanup | Membrane semantic policy |
| `membrane-client` | Stateless stdio/hook/CLI transport, installed activation & harness lifetime holder | Planner, store, watcher, embedder or direct-store fallback |
| Shared engine | One runtime identity, planner, embedder, stores, watchers, MCP/API execution, drain & shutdown | Host transcript compaction |
| Pull | Retrieval, provider admission, selection, reduction, publication & exact recovery | Durable truth ownership |
| Cortex | Durable knowledge admission, immutable pushed-memory source, lifecycle, storage & retrieval | Final attention policy |
| Host | Invokes hook/MCP, inserts returned context, enforces host-native effects | Evidence selection or permission expansion |

One owner generates each host-facing artifact. One MCP server name exists per host configuration scope.

## 4. Installed layout

```text
Membrane/
  versions/<release>/
    membrane.exe
    membrane-client.exe
    membrane-hub.exe
    membrane-tray.exe
    plugin/
      plugin.json
      mcp.json
      skills/membrane/SKILL.md
      hooks/
      host projections
    runtime/
    release.json
  current/                 exact active payload
  state/
    activation-receipt.json
    install-events.jsonl
```

All plugin identity/version fields derive from release identity. No hand-maintained Claude/Codex version copies. Host launch paths resolve through stable `current`, never repository, `dist`, Cargo target, pnpm output or version directory.

## 5. Host projections

| Host | Plugin/discovery | MCP transport | Startup/lifetime |
|---|---|---|---|
| Codex | Portable Agent Plugin with OpenAI metadata, skill & native hook | Authenticated Streamable HTTP to installed engine | Trusted SessionStart starts/adopts & acquires harness holder; direct HTTP session retains access |
| Claude Code | Native Claude plugin with skill & native hook | Authenticated Streamable HTTP to same engine | SessionStart starts/adopts & acquires harness holder |
| Devin/Windsurf | Owned host projection | Stable absolute `membrane-client stdio-mcp` until direct authenticated HTTP is separately proven | Client starts/adopts, forwards & holds same engine |
| Cursor | Owned host projection | Stable absolute `membrane-client stdio-mcp` until direct authenticated HTTP is separately proven | Same client contract |
| Antigravity | Native projection | Stable absolute `membrane-client stdio-mcp` until direct authenticated HTTP is separately proven | Same client contract |
| CodeRight | Existing signed native integration | Authenticated HTTP connection pool | Starts/adopts & holds same engine; no redundant operation health exchange |

Codex & Claude never use `membrane stdio-mcp`. `membrane.exe` owns installed engine/control modes; `membrane-client` owns stdio, hook & CLI forwarding.

Portable `mcp.json` declares `streamable-http` for Codex. Plugin-scoped host configuration supplies bearer-token environment binding without creating a second global MCP registration. Claude native projection declares matching HTTP URL/header contract. A host adapter that cannot express required authenticated HTTP uses a host-specific stdio projection instead of loading portable MCP declaration plus manual duplicate.

## 6. Shared-engine lifecycle

- Exactly one installed engine exists per OS user & installed state.
- Hub on starts/adopts & holds engine regardless of active harnesses.
- Harness access starts/adopts & holds engine regardless of Hub state.
- SessionStart is Codex/Claude bootstrap path; stdio client is stdio-only host bootstrap path.
- Socket reuse is not ownership. Each holder has authenticated identity, renewal/loss semantics & bounded release.
- Closing one owner never stops engine held by another.
- Final owner release drains work & stops engine.
- Process loss or lease expiry releases that holder.
- Recovery restarts engine only while valid owner remains.
- No ownerless engine autostart, scheduled restart, alternate port/store or one-shot runtime fallback.
- Background watcher/mining authorization remains separate from harness access.

## 7. Plugin & activation transaction

Activation must:

1. Verify release manifest, payload digests, executable identity & one coherent plugin version.
2. Discover each supported host through its native mechanism.
3. Stage complete host projection before mutation.
4. Install or repair plugin registration where host supports plugins.
5. Enable plugin where policy permits; otherwise return explicit required-user-action state.
6. Install exactly one MCP binding with exact transport, URL/command & credential source.
7. Remove only obsolete Membrane-owned duplicate bindings.
8. Install exact native hooks without claiming trust or observation until proven.
9. Initialize MCP, list tools/resources & execute harmless status against exact installed generation.
10. Commit receipt only after requested host boundaries pass.

Failure restores prior `current`, prior working host projection, prior registration & prior hooks. Uninstall removes only exact Membrane-owned entries/files & preserves unrelated settings.

## 8. MCP contract

Default agent-facing tools remain intentionally small:

| Tool | Contract |
|---|---|
| `pull` | Retrieve smallest sufficient current context across admitted Cortex, Blueprint, Ledger & exact-source evidence; return authority, freshness, coverage, omissions, reduction, recovery & accounting receipt. |
| `push` | Write agent-submitted durable memory through Cortex admission; preserve submitted body byte-for-byte in immutable source/admission record. It is not context reduction. |

Resources & prompts remain grant-bound. Tool discovery does not imply repository authorization. Invalid scope returns typed denial without content leak. `push` adds no Blueprint, Ledger or Adapt write destination without another accepted decision.

Status must report:

- installed release, stable root, resolved version & generation;
- plugin installed/enabled state per host;
- hook installed/trusted/observed state;
- MCP configured/initialized/listed state;
- engine identity & holder state;
- repository/grant state without leaking protected content;
- Pull provider readiness/omissions;
- public `push` Cortex-write availability;
- exact missing prerequisite for every unavailable operation.

## 9. SessionStart contract

Codex & Claude native SessionStart invoke stable `membrane-client hook` once per start/resume/clear/compact event supported by host.

Hook:

1. validates host envelope & installed identity;
2. starts/adopts shared engine & acquires holder;
3. requests one bounded orientation Pull from shared planner;
4. returns repository identity, Blueprint generation/freshness, Cortex knowledge, Ledger evidence, Adapt-derived admitted knowledge, provider availability & typed omissions;
5. never builds/updates Blueprint, dumps stores or bypasses grants;
6. emits no duplicate packet for same host event identity.

## 10. Acceptance

| Boundary | Required proof |
|---|---|
| Package | Root manifest, MCP definition, skill, hooks & host projections share release version/digest; every declared path exists. |
| Codex plugin | Appears installed/enabled in actual Codex; skill is discoverable; hook trust/observation is accurate. |
| Claude plugin | Appears installed/enabled in actual Claude Code; skill & native hook are discoverable. |
| Registration | Exactly one `membrane` MCP connection exists per scope; no plugin/manual duplicate. |
| Transport | Codex/Claude use authenticated HTTP; stdio hosts use exact stable `membrane-client`. |
| Wake | Hub-off host access starts one engine; second host adopts it; no duplicate process/store/embedder/watcher. |
| Ownership | Releasing one holder preserves remaining owner; final holder release drains & stops engine. |
| MCP | Initialize, list tools/resources & call status against exact installed generation. |
| Authorization | Ungranted call returns typed denial; enrolled caller succeeds without changing transport. |
| Push | Independent pushed body reads back byte-identically from Cortex source/admission record with correct scope/provenance. |
| Pull | Broad task retrieves pre-existing eligible evidence across available providers; failure/timeout feeds typed omissions rather than an echo-memory test. |
| SessionStart | One bounded packet appears through real host event with correct repository/generation/provider state. |
| Repair/update | Reconcile remains idempotent, preserves unrelated config & rolls back failed activation. |
| Uninstall | Owned plugin/MCP/hooks/runtime are removed; unrelated host state remains. |

Schema validation alone never satisfies these boundaries.

## 11. Current known defects

- Packaged plugin files are present under installed `current` but are not installed/enabled in Codex or Claude.
- Portable MCP manifests call `membrane stdio-mcp`, which installed engine rejects.
- Claude plugin invokes `membrane.exe` for MCP & hooks instead of direct HTTP plus `membrane-client hook`.
- Codex & Claude plugin versions drift; root portable manifest lacks coherent release version.
- Activation currently registers MCP/hooks but does not complete native plugin installation.
- Current activation receipt can say engine `ready` while `clients` is empty.
- Schema validator accepts manifest structure without executing declared transport.

## 12. Decision-driven atom reconciliation

Preserve IDs & history. Reword changed atoms, add explicit lineage for splits/merges, then reconcile implementation/verification/qualification against live source.

### Membrane atoms to change

| Atom | Decisions | Required change |
|---|---|---|
| MEM-006, MEM-007 | 20, 21, 24, 25 | Replace tray-owned-child wording with Hub/tray start-or-adopt holder of one shared installed engine. |
| MEM-008 | 20, 21, 24 | Replace “explicit operations never start residency” with harness access starting/adopting shared engine while Hub is off. |
| MEM-009 | 20, 22, 24 | Replace one-shot/never-start semantics with CLI/MCP transport into shared owner plus authenticated holder acquisition. |
| MEM-016 | 20, 21, 24 | Report shared engine identity, all holders, final-owner drain & typed unavailable state; remove tray-exclusive ownership. |
| MEM-025, MEM-026 | 17, 18 | Remove six-subsystem/Push-subsystem inventory claims; report five subsystems plus public `push` write operation. |
| MEM-045 | 7–9, 22, 24 | Keep Claude capability atom; require native plugin installed/enabled, direct HTTP MCP, SessionStart & actual-host proof. |
| MEM-046 | 7–9, 22, 24 | Keep Codex capability atom; require installed/enabled Agent Plugin, direct HTTP MCP, plugin-scoped auth, SessionStart & actual-host proof. |
| MEM-047 | 20, 23, 24 | Replace “same tray daemon” with same shared installed engine & CodeRight holder/pool contract. |
| MEM-048 | 20–22, 24 | Keep Cursor/Windsurf projection; require exact stable client, one registration & actual-host proof. Devin must not be inferred from Windsurf config compatibility. |
| MEM-054 | 17, 18, 20 | Remove Push as subsystem identity; bind Pull delivery/public-push-write identities separately. |
| MEM-055 | 21, 24, 25 | Replace ownerless tray supervision with bounded recovery only while valid Hub/harness owner remains. |
| MEM-056 | 21, 24, 25 | Drain/stop only after final owner loss or explicit governed stop; one owner closing cannot terminate shared engine. |
| MEM-067 | 7–9, 24 | Retain SessionStart atom; bind it to plugin installation, holder acquisition, shared Pull & no graph/store construction. |

### New Membrane atoms required

| Proposed ID | Decisions | Observable behavior |
|---|---|---|
| MEM-068 | 18, 19 | Expose public `push` as grant-bound agent-to-Cortex durable-memory write only, with typed result & no reduction/subsystem ambiguity. |
| MEM-069 | 20–24 | Project Membrane into Devin through one proven native/compat installation path with exact plugin/MCP state, stable executable & no capability inference from Windsurf visibility. |

### Blueprint atoms to change

| Atom | Decisions | Required change |
|---|---|---|
| BPT-021 | 3–5 | Every update to valid graph is incremental; cover watcher event path, scan/diff, hash confirmation, affected-reference repair, no-change exit & content-addressed reuse. |
| BPT-042 | 20, 24 | Remove bounded one-shot direct runtime; all CLI/SDK/MCP adapters reach same shared engine/Blueprint owner. |
| BPT-043 | 4, 21, 24 | Watcher maintenance runs from active Hub or CodeRight holder; explicit user build/refresh remains independent of Hub. |
| BPT-056 | 1, 2 | Full construction occurs only when graph is absent or proven corrupt/unrecoverable; newer/incompatible graph is preserved, migrated or typed-rejected. |

Add BPT-072 for decision 6: context/query/recall/status/freshness reads never construct or update graph.

### Cortex atom required

Add CTX-043 for decisions 18–19: accepted public-push memory preserves submitted body byte-for-byte in immutable source/admission record; embeddings, metadata, lifecycle & derived representations never replace it. CTX-003/CTX-004 remain supporting write/admission atoms, not substitutes.

### Relevant Membrane atoms that remain & require closure

- Authority/identity: MEM-003–MEM-005, MEM-018–MEM-021 & MEM-042.
- MCP contract: MEM-010–MEM-015, MEM-017, MEM-043 & MEM-049–MEM-050.
- Honest host state: MEM-031, MEM-045–MEM-048 & MEM-067.
- Transactional delivery: MEM-057–MEM-060.
- Changed lifecycle atoms above remain committed after rewording; decision correction does not close them.

No listed Membrane capability is lifecycle-closed until focused verification, installed-host qualification, required delivery & noncontradictory evidence all pass.

### Pull & retired Push atoms

Pull remains owner for retrieval, selection, reduction, publication & recovery. Existing PUL-001–PUL-033, PUL-035–PUL-042 remain relevant. PUL-011, PUL-012 & PUL-015 retain Cortex, Blueprint & direct Ledger provider boundaries. PUL-027, PUL-028, PUL-031, PUL-035 & PUL-042 remain incomplete owners for migrated reduction/recovery work.

Push canon cannot remain 29 committed peer-subsystem capabilities while its header calls subsystem retired:

- Migrate PSH-003–PSH-004, PSH-006–PSH-007, PSH-009–PSH-017, PSH-019, PSH-026 & PSH-027 into Pull delivery/reduction lineage.
- Migrate PSH-002, PSH-005, PSH-022–PSH-025, PSH-028 & PSH-029 into Pull recovery/artifact lineage.
- Move PSH-001, PSH-008, PSH-018, PSH-020 & PSH-021 to host/CodeRight integration lineage or explicit legacy compatibility; they are not Pull-owned universal host interception.
- Preserve every PSH ID as historical alias/reference. Remove retired Push group from committed capability totals only after complete migration map proves no lost behavior.

### Ledger, Adapt & Cortex decisions already represented but still open

- LDG-001–LDG-006, LDG-010–LDG-012, LDG-014–LDG-020, LDG-022 & LDG-024–LDG-031 remain relevant; LDG-022 is direct Pull-provider boundary.
- ADP-029 correctly routes accepted Taste/Insight through Cortex; ADP-035 correctly keeps mining proposal-only/background. Remove residual direct Adapt provider wiring & update ADP-030 “Push facts” terminology.
- CTX-001–CTX-017, CTX-020–CTX-030 & CTX-032–CTX-042 remain Cortex authority/lifecycle/retrieval work. CTX-033 stays exploratory unless separately promoted.

## 13. Closure order

1. Reconcile atom wording/lineage against accepted decisions; regenerate pending index/counts.
2. Generate one coherent plugin package/version & correct HTTP/client/hook manifests.
3. Implement native Codex/Claude plugin installation, enablement, trust/status & one-registration ownership.
4. Prove shared-engine holder lifecycle through Hub-off, multi-host & final-owner shutdown tests.
5. Prove public `push` exact Cortex write & broad pre-existing-evidence Pull.
6. Run actual installed Codex, Claude & Devin acceptance; bind receipts to release identity.

## References

- `docs/architecture/adr/2026-09-12-context-system-decisions.md`
- `docs/architecture/single-instance-membrane.md`
- `docs/architecture/membrane.md`
- `docs/canon/membrane.md`
- `docs/canon/pull.md`
- `docs/canon/push.md`
- `docs/canon/cortex.md`
- `docs/canon/blueprint.md`
- `docs/canon/ledger.md`
- `docs/canon/adapt.md`
- OpenAI Plugin packaging: <https://developers.openai.com/plugins/build/plugins>

**Done means each supported host visibly owns one correct plugin/MCP projection, reaches one shared authorized engine, executes real Pull/public-push behavior & reports every intermediate state honestly.**
