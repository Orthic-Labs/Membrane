# Membrane final implementation contract

**Status: accepted target architecture, 2026-09-16.** Adrian explicitly directed amendment of canons to implement this final shape. This contract governs scope & execution together with current capability, implementation & qualification ledgers. It is not a recommendation list or runtime completion claim.

## Start here

1. Read this contract, [accepted context decisions](../architecture/adr/2026-09-12-context-system-decisions.md), then affected [canons](README.md).
2. Implement every applicable `COMMITTED` behavior through its real production consumer. Reuse sound existing mechanisms; inspect implementation & qualification gaps before choosing repair.
3. Do not implement `EXCLUDED` rows as required product scope. Existing code or an older plan does not promote them. Preserve necessary compatibility & safety until migration acceptance passes.
4. Use [pending state](../pending/README.md) for current progress. Earlier pending plans, donor intakes & receipts preserve history; they cannot override current scope or revive superseded ownership.
5. Close only exact capabilities proven through their required installed/released boundary. One end-to-end run may prove several named atoms; a docs change, focused test, implementation flag or competitive ranking does not establish lifecycle closure.

This amendment supersedes older requirements for a peer Push subsystem, unconditional host-capacity refusal, independent Blueprint self-update, Cortex ownership of skill-document indexing, Ledger session projections, permanent reserved memory/skills lanes, & mandatory implementation of rows now outside committed scope. All unaffected committed requirements remain required; this is the final target, not a reduced first-release checklist.

## Required ownership

| Owner | Required responsibility | Boundary |
|---|---|---|
| Membrane | One planner, authenticated shared-engine lifetime, transport, installation identity, receipts, scheduling & host integration | No alternate client runtime/store; Hub or accessing harness holds engine; final holder loss drains/stops it |
| Pull | Authorized retrieval, eligibility, selection, coverage, budget fitting, faithful delivery & exact recovery | One final planner; source-local scores/caps are proposals; no universal host-transcript control |
| Blueprint | Live repository evidence, source/generation identity, incremental graph maintenance & graph compatibility | Reads never build/update; full construction only absent or proven corrupt/unrecoverable graph |
| Cortex | Admitted durable knowledge, scope, conflicts, lifecycle, recall, erasure & recoverable storage | Public `push` stores submitted body byte-for-byte; reviewed Adapt knowledge enters here |
| Ledger | Source-bound document & portable skill-document projections, index, invalidation & exact resolution | Source files remain authoritative; no ordinary document/body replication into Cortex; no session-store expansion |
| Adapt | Evidence-bound mining, reviewed Taste/Insights, lineage, proposals & outcome interpretation | No direct Pull injection, independent durable-truth owner, host experiment runner or guard enforcement |
| Host / CodeRight | Invocation, insertion, owned tool-output capture, execution & enforcement | Membrane adapters preserve explicit compatibility contracts without becoming a general command runner |

Native Codex/Claude HTTP integration, thin generic stdio, CodeRight's existing native integration, committed Cursor/Windsurf projection & independently proven Devin projection remain required. Host-specific proof never transfers by assumption. Preserve public V1 compatibility through existing negotiation/versioning; do not invent a second protocol authority.

## Scope & retained mechanisms

Current `Scope` cells are authoritative & binary: `COMMITTED` means required; `EXCLUDED` means not required. There is no deferred-work queue. Excluded IDs preserve history, replacements & safe retirement obligations; they are not future implementation instructions.

| Area | Final disposition |
|---|---|
| Working-context envelopes, scratchpad expansion, notifications, diagnostic baselines, team-policy overlays | MEM-022/023/032/038/051 are excluded. Basic health, traces, diagnostics, support bundles & scheduling safety remain committed. |
| Generic stdio duplication | MEM-049 is an excluded historical alias of MEM-011. Preserve generic-client acceptance under MEM-011. |
| Client libraries | MEM-050 commits shared transport-contract compatibility. Separately distributed SDK products are not required. |
| Alternate fusion & optional reduction | PUL-021/039/040/044/045/047/048 are excluded. Preserve current fixed-order control only until qualified default-policy migration; no second permanent planner. |
| Semantic memory maintenance & causal experiments | CTX-023/024/031 are excluded; CTX-033/039/042 are excluded. Existing vector/hybrid recall, bounded recipes, operational events, lifecycle & restore remain committed. |
| Skill documents | CTX-034 is excluded as a historical alias of LDG-032. Reuse existing implementation through Ledger-owned adapters; logical ownership correction does not mandate physical storage relocation. |
| Ledger session projections | LDG-021/025 are excluded. Retire production promotion paths; preserve fail-closed non-recallability until retirement. LDG-016/027 are excluded; LDG-023 is excluded. |
| Broad Adapt detector/optimizer expansion | ADP-020/041/043–052/055–056/060–061/063–064/076–077 are excluded. ADP-065–071 are excluded. Existing narrow detectors, review, lineage, debug inspection & honest pipeline status remain committed. |
| Blueprint findings & providers | Retain existing findings, explanation, export, HTTP/SQL/framework/Terraform & explicit cross-language evidence capabilities. Qualify existing consumers; do not add speculative domains. BPT-048 is excluded. |

Excluded optimizations must not become implicit dependencies of committed capabilities. Native exact content, faithful excerpts & authorized exact resolvers form Pull's required baseline. Excluded representations are not new implementation work; preserved legacy use keeps all authority, fidelity, recovery, quota & cancellation protections.

Operational event storage (CTX-032) stays distinct from authored knowledge & continues supporting existing daemon/review consumers. Adapt lineage (ADP-042) describes evidence-to-proposal/outcome history, not repository semantics. ADP-075 must expose empty workload, missing bindings, blocked/failed work & unavailable outcome joins honestly.

## Blueprint availability & freshness

Existing valid Blueprint graph evidence remains available to Pull even when stale, labelled with its actual generation & freshness. Staleness alone is not an exclusion. Authorization, schema compatibility, generation integrity, exact source-resolution checks & explicit current-evidence requirements remain enforced. Hub/CodeRight-owned watchers or explicit build/refresh operations maintain freshness; Pull reads never update graphs. Qualify stale-graph delivery independently from automatic refresh after edits, commits & restart.

## Pull budget contract

PUL-050 supports two explicit modes across native, HTTP & stdio projections:

- **Bounded-response:** require a positive bounded caller/configured response budget with declared estimator/unit; fit complete rendered response including wrappers & receipts. Missing host remaining capacity is reported as unknown, not treated as failure. This mode never guarantees whole-transcript fit.
- **Host-fit:** require trusted, fresh, exact & identity-matched request-time H8 host remaining-capacity evidence. Fit the tighter applicable response/host ceiling. Missing, stale, inexact, malformed or mismatched H8 returns typed refusal; never silently downgrade modes.

For compatible legacy requests without an explicit mode, supplied H8 retains host-fit semantics; otherwise use bounded-response under explicit caller/configured response budget. Invalid supplied host evidence is never silently discarded to obtain success. Neither mode invents host capacity, widens grants, hides omissions, drops protected spans or bypasses recovery requirements.

PUL-027 selects eligible qualified representations under that mode. PUL-028 reports response budget separately from observed/unknown host capacity. PUL-031 records requested/effective mode, estimator, budget provenance, observations, omissions & actual delivery. PUL-051 applies the same mode to every fallback; protected evidence that cannot fit yields typed insufficiency/refusal. Historical PSH host-fit tests remain required for host-fit mode, not a universal requirement to observe private host state.

PUL-022 supplies the named/versioned RRF target default; compare it against preserved PUL-021 fixed-order control before switching. PUL-026 closes only after evidence-class policy qualifies & reserved memory/skills lanes leave the normal path. Rollback controls may remain explicitly isolated; closure must not require maintaining two production policies indefinitely.

## Installation & host-execution migration

BPT-058/061/062/063 become Blueprint projections into canonical Membrane installation transactions (MEM-021/057–060). Blueprint reports graph/schema compatibility & rejects unsafe transitions; installer owns staging, trust admission, activation, rollback & release artifacts. No independent Blueprint self-updater is part of final shape.

MEM-070–074 apply only to named host/CodeRight integrations or explicit legacy compatibility adapters. Before retiring a runner, enumerate live consumers & migrate or explicitly retire each entry point. While any runner remains, command grammar, confinement, direct argv, environment restrictions, capture-once & exact recovery stay mandatory. No unsupported claim of Codex/Claude external tool interception or transcript compaction is permitted.

MEM-053 owns proposal-job transport, event cursor & sink handoff; MEM-052/061–066 own generic scheduling. ADP-035 owns learner semantics, Cortex owns durable admission. Reuse these owners; do not build another semantic review engine or proposal database.

## Execution order & acceptance

These are integration milestones over existing atoms, not additional countable capabilities. Independent work may proceed in parallel; dependencies govern acceptance.

| Milestone | Required observable result | Principal atom owners |
|---|---|---|
| 1. Installed vertical path | Supported host activates/adopts one engine; receives bounded orientation; calls Pull; resolves exact evidence; pushes memory; recalls exact submitted source; releasing final holder stops engine | MEM-006–017/045–047/055–056/067–069; PUL-011/012/015/027/035/042/043/050; CTX-043; LDG-022 |
| 2. Source & storage correctness | Authorized multi-source context survives edits, stale/denied inputs, interruption & restart; graph reads remain nonmutating; durable memory conflict/erasure/restore & resolver expiry remain honest | BPT-021/056/072; Cortex admission/lifecycle/backup/restore; Ledger identity/invalidation/resolution; Pull authority/accounting/recovery |
| 3. All promised hosts & updates | Each committed host independently proves installed access, discovery, invocation & delivered context; update interruption/rollback preserves app/store compatibility & one engine identity | MEM-018–021/031/045–050/054/057–060/067/069; BPT-058/061–063 |
| 4. Useful Adapt loop | One real recurring issue produces inspectable evidence, reviewed Taste/Insight, Cortex admission & relevant later recall; duplicate/stale/rejected proposals cannot mutate truth; pipeline failures remain visible | ADP-003–019/023–030/034–036/038/040/042/072–075; PUL-011; Cortex admission |
| 5. Remaining committed qualification | Retained diagnostics, providers, findings, exact detectors & other committed behavior meet their individual acceptance gates; no excluded work is required to claim committed completion | Every remaining committed capability & matching qualification row |

For any later optimization proposal, compare equal-budget useful evidence/task outcomes, missing or wrong evidence, latency, token/storage cost, false positives & recovery cost against current qualified behavior. Preserve authority & source fidelity as hard invariants. More findings, lower token counts or more graph edges alone do not establish improvement.

## Completion rule

Implementers report exact committed atoms reached, their production consumer & required-boundary evidence; report gaps honestly. Preserve stable IDs, old wording, aliases & prior receipts. New scope or changed semantics invalidates affected proof only. Never shrink required scope merely to claim closure or reactivate excluded work to satisfy a historical plan.
