# CodeRight + Membrane — independent engineering assessment

**Status: research. Not canon, not architecture, not a progress ledger.** It does not change any
atom state, closure count, or ownership. Where it disagrees with canon it says so explicitly and
proposes an amendment; it never silently overrides one.

**Inspected heads (fetched 2026-09-08):**

| Repository | `main` SHA | Head commit |
|---|---|---|
| `bogusyogi/coderight` | `334699e5120175cd151c1b8d9ecff7d52c8f96e8` | fix: advance engine host and scheduler contracts |
| `Orthic-Labs/Membrane` | `2e8e57bbb1e368332b37466080f1e925f2f4726e` | checkpoint: propagate workspace deadlines and bound target fanout |

All paths and line numbers below are at those SHAs.

---

## 1. Direct assessment

**What is already sound.** The execution half of CodeRight is in better shape than its canon
counters suggest. Approval-to-payload digest binding (`EFF-002`) is fully implemented and closed at
source — approval binds the post-parse normalized digest, execution recomputes and compares it, the
grant is single-use, and `PreToolUse` hooks are deny/proceed only, so there is no mutation window
between approval and execute. The `EVT-004/005/006`, `KRN-001/002/003`, `HCTX-003` and
`SRCH-001/002` closures carry real executed runtime receipts with independent validation — process
topology sampling, durable-event readback from independently reopened SQLite, real PTY, real
provider streams. That is a genuinely high evidence bar, and it is being met. On the Membrane side,
Pull's provider model is well-built: typed omissions, deadline propagation, and a deliberately
fail-closed `BlueprintProvider::new` that refuses to dispatch without a request-aware source.
Blueprint's evidence coordinates are exact — I independently verified `path`/`startLine`/
`contentHash` against source bytes. Graph construction is genuinely fast and consistent: across two
replicated runs each, 190 `.mjs` files → 4,669 nodes in 415–506 ms in memory and 1,850–1,893 ms
persisted; 200 `.swift` files → 2,240 nodes in 365–442 ms and 1,469–1,530 ms. Build cost is not a
problem in either system.

**What is genuinely missing.** A working link between the two products. CodeRight's entire
consumption surface for Membrane is a Cortex memory CRUD/recall trait
(`engine/crates/coderight-memory-backend/src/contract.rs`, `trait MemoryBackend`). Blueprint, Pull,
Ledger, Push and Adapt reach a CodeRight turn through nothing at all: not the automatic context
path, not the tool catalog, not a default MCP registration. The automatic context a CodeRight agent
receives is five Cortex memory strings selected by `recall_scored(query = raw user text, limit = 5)`
at message-admission time. Everything the brief calls "repository intelligence" is, for CodeRight
users today, whatever the model gets from `read_file`, `search` and `lsp`.

**What is present but ineffective or unproven.** Three things. (a) `context_federation.rs` — 1,554
lines implementing the H8 ceiling, packet reduction, delivery lanes and receipts — is reachable
only through an HTTP route whose only caller in the whole repository is a test, and that route is
mounted only by ServiceHost, while `KRN-001/003` made InlineHost the default local host. (b)
CodeRight ships a second repository map (`blueprint_init.rs`) that walks the user's whole tree on
every session and writes into their working directory, and nothing ever reads it back. (c)
Blueprint's shipped retrieval quality evidence is 30 cases over four fixture repositories totalling
18 files, measured against `queryGraph` and a lexical-only build — not against the production
recall path and not against the shipped tree-sitter provider.

**Greatest opportunity.** Blueprint's production seed resolver cannot find a camelCase symbol by its
own name. On CodeRight's own desktop app it resolves the correct symbol as a seed in **3.2%** of
exact-identifier queries and returns it in the top 10 in **17.1%**. A two-line change to
`seed-resolver.mjs` takes that to **68.2% / 59.2%** with no context bloat. This sits on
`blueprint_recall` — the one call CodeRight's own agent rules tell agents to make *before reading
repository files*. Fixing it is the single highest-leverage change available in either system, and
it is smaller than any of the integration work queued behind it.

---

## 2. What "better" means here

Reconstructed from `docs/canon/daemon-master-canon-atoms.md`, `docs/canon/README.md`,
`docs/agent-rules.md` (CodeRight) and `docs/architecture/membrane.md`,
`docs/architecture/execution-lifecycle-boundary.md`, `docs/canon/*.md` (Membrane).

The system's own stated hierarchy is explicit and I adopt it: **authority, safety, correctness,
durability and replay truth are hard constraints; token efficiency, latency, CPU/RAM/process count
and UX are optimized inside them** (master canon, "Optimization priority correction"). Ownership is
also a hard constraint: CodeRight owns execution, Membrane owns semantic context and repository
intelligence, and neither may grow a second authority.

Concretely, a change is an improvement when it increases the share of real coding tasks completed
correctly on the first attempt, by getting relevant and current evidence to the model at the moment
it must decide, without adding an authority, a process, or an unproven claim. Fewer tokens that cost
the agent a necessary fact are not an improvement. More context that does not change a decision is
not an improvement either.

Two consequences shape my ranking. First, a retrieval miss is worse than a retrieval cost: the agent
recovers from a slow call, but a false negative sends it to read whole files or, worse, to invent.
Second, an availability failure that blocks *every* turn outranks any quality improvement.

---

## 3. End-to-end map

```
CodeRight turn (current, default InlineHost)
  user text
    -> api_sessions/messages.rs:307   require_root_membrane_binding()  [hard gate]
    -> api_session_context_runtime.rs:227 retrieve_session_memory(text, limit=5)
         -> MemoryBackend::scopes / recall_scored / record_injections   [HTTP -> Hub]
         -> MemoryProvenanceStore::effective_for  x2 per item           [HTTP -> Hub]
    -> dynamic_user_turn_reminder(<workspace_memory> block)
    -> engine_loop -> provider dispatch
         (no Membrane call here; no ContextPacket; no receipt)
    -> tools: read_file | search | lsp | edit_file | shell | ...
    -> engine_tool_dispatch_approval  (digest bound)
    -> engine_tool_dispatch_execute   (digest rechecked, single-use)
    -> SQLite durable events -> projections

Membrane (what exists, and who reaches it)
  MCP host (Claude Code, Codex)  --membrane_context--> /federate -> Pull federation
                                                          -> BlueprintProvider -> blueprint recall
                                                          -> Cortex / Ledger / skills / freshness
                                                          -> admission -> ContextPacket + Receipt
  CodeRight                      --/recall,/get,/scopes--> Cortex memory only
  CodeRight ContextFederationClient --/federate--> (route exists; no production caller)
```

The asymmetry is the finding: Membrane's flagship native integration partner has strictly less
access to Membrane than a generic MCP host does.

### Coverage record

| Area | How I covered it |
|---|---|
| Blueprint graph build, `queryGraph`, `resolveSeeds`, `executeRecallCircuit`, `recallCircuitToCandidateSet`, tree-sitter augmentation | **Runtime-tested by me** on two real repositories and the shipped 30-case corpus, in an isolated container; ground truth derived from source text, not from the retriever |
| CodeRight Membrane binding, context injection, tool registry, effect/approval path, `blueprint_init` | **Source-inspected**, with call-site enumeration across the whole repository |
| Membrane explicit (Hub-off) client/protocol/server and CLI wiring | **Source-inspected** end to end, including argv dispatch |
| CodeRight `EVT-*`, `KRN-*`, `HCTX-003`, `SRCH-*` closures; Hub-off `membrane_timeout_known` probe | **Evidence-backed** from the team's own recorded RightKit receipts in canon |
| CodeRight Rust build/tests, installed Membrane binary, Hub lifecycle, MCP server runtime, desktop, iOS, provider dispatch, PTY, installer, real model runs | **Unverified.** Not built or executed here. No latency, token or memory numbers are claimed from source inspection |

I did not attempt a CodeRight Cargo build. It is a 556k-line workspace whose qualification target is
Windows-installed, the assessment forbids production edits, and product-scope rules forbid widening
build scope from a shared dependency. Nothing below depends on a claim I could only have made by
building it.

---

## 4. Verified defects

### D1 — Blueprint seed resolution destroys the identifier it is given (Membrane)

**Problem.** `blueprint/src/graph/seed-resolver.mjs:6-9` splits camelCase and lowercases the query
*before* the exact-equality lanes at `:78` (`qualified_name = ?`), `:86` (`name = ?`) and `:96`
(`symbol_terms.token = ?`). The stored columns are case-preserved and hold the whole identifier. So
a query for `assertPublicationCandidate` is decomposed to `assert`, `publication`, `candidate` and
matched against symbols *literally named* `candidate`. The symbol asked for is never a candidate.

Observed directly (`executeRecallCircuit` on `blueprint/src` itself):

```
TASK assertPublicationCandidate  state=complete seeds=5 paths=5
  seeds: bm25-code-index.mjs::candidate, language-extractors.mjs::candidate,
         update/manifest.mjs::candidate, javascript.mjs::candidate, git-metadata.mjs::candidate
TASK executeRecallCircuit        state=complete seeds=2
  seeds: admission.mjs::recall, application/service.mjs::circuit
TASK resolveSeeds                state=complete seeds=1
  seeds: findings/detect.mjs::resolve
```

The `symbol_terms` index does not carry the whole identifier either — tokens stored for
`assertPublicationCandidate` are `assert, candidate, publication, graph, mjs, policy,
publication_policy` — so the `bounded_lexical` lane cannot rescue it.

**Affected workflow.** `blueprint recall` is the production path: JS
`blueprint/src/lib/application/service.mjs:561-585` → consumed by the resident service, the
`membrane_blueprint` MCP tool, and the Rust Pull lane via
`engine/crates/membrane-federation/src/blueprint_client.rs:264` (method `"recall"`). CodeRight's own
`docs/agent-rules.md` instructs agents to call `blueprint_recall` with the repository root *before
reading repository files*. This is the first retrieval an agent makes.

**Measurement.** Ground truth is exported functions with a leading doc comment, extracted by regex
over source; the retriever is never asked to validate itself. Top-10 containment of the correct
`path` in the delivered candidate set.

All "variant" figures are the combined D1+D2 patch in `probes/fixed/`. D1 alone accounts for almost
all of it: on `blueprint/src`, D1 alone gives 68.3% / MRR 0.549 and D2 adds the remaining 2.4 points.

| Repository | n | current | variant | seed-stage discovery, current → variant |
|---|---:|---:|---:|---|
| `Membrane/blueprint/src` (JS, camelCase) | 164 | 32.9% (MRR 0.194) | **70.7%** (MRR 0.585) | 9.1% → **81.7%** |
| `coderight/apps/coderight-tauri/src` (TS, camelCase) | 152 | 17.1% (MRR 0.096) | **59.2%** (MRR 0.462) | 3.2% → **68.2%** |
| `coderight/apps/coderight-ios/CodeRight` (Swift, camelCase) | 400 | 52.3% (MRR 0.354) | **67.8%** (MRR 0.509) | — |
| `coderight/engine/crates/coderight-memory-backend/src` (Rust, snake_case) | 123 | 81.3% (MRR 0.726) | 86.2% (MRR 0.839) | — |

**Scope caveat, stated plainly.** The defect is *identifier-shape specific*. `terms()` keeps `_` in
its token character class, so snake_case identifiers survive intact and Rust is only mildly affected
(+4.9 pts). Severity tracks how much of an identifier is destroyed by the camelCase split:
TypeScript (+42.1 pts) and JavaScript (+37.8 pts) are worst; Swift sits between (+15.5 pts), because
many Swift methods are short single words that survive the split. So CodeRight's own Rust engine work
is barely affected, while its desktop app, its iOS app, and any JS/TS/Java/C#/Kotlin/Go user are
affected materially to severely. Larger graphs are worse: the 10,007-node TS graph scored half what
the 4,669-node JS graph did, consistent with generic fragments (`create`, `state`, `use`) saturating
the exact lanes at scale.

**Root cause.** Normalization intended for *scoring* was applied to keys used for *exact equality*.
`queryGraph`, which substring-scores instead, finds the same symbols at 99.4% — so the graph
contains the answer; only the lookup discards it.

**Owner and boundary.** Membrane / Blueprint. No CodeRight change, no new authority, no new
subsystem. It is a change to `seed-resolver.mjs` only.

**Mechanism.** In `terms()`, emit the whole query token alongside its camelCase fragments; compare
the `qualified_symbol` and `exact_term` lanes case-insensitively (`lower(qualified_name) = ?`,
`lower(name) = ?`, with an index to match). Preserve lane order and exactness ranks exactly as they
are so `comparePaths`' non-compensatory ordering is untouched.

**Benefit / uncertainty / cost.** Benefit measured above on two real repositories, same direction,
large effect. Uncertainty: whether the delivered gain survives Pull admission and packet budgeting
(untested here); whether case-insensitive lanes introduce new ambiguity in languages with
case-distinguished siblings (Go `Foo`/`foo`) — the existing `ambiguous` state already covers this
and should be checked. Effort: hours. Regression risk: low, but non-zero, because it changes which
seeds resolve and therefore which paths are returned; the existing recall contract tests must be
re-run.

**Smallest experiment.** Already run and reproducible: build the graph for a camelCase repository,
run `executeRecallCircuit` for each exported symbol's own name, measure top-10 containment before
and after. **Accept** if exact-identifier containment on a camelCase repository at least doubles and
snake_case and the 30-case corpus do not regress. **Reject** if ambiguity rates rise enough to push
`state` to `ambiguous` on previously-resolving queries.

**Canon amendment proposed.** `BPT-023` (`docs/canon/blueprint.md:38`) currently reads: *"Resolve
Recall seeds through valid ID, source/path/anchor, qualified symbol, exact term, bounded lexical,
else abstain/ambiguous."* Its implementation satisfies that sentence while failing its purpose, and
its competitive state is recorded as `DONOR_BETTER` — framed as a comparison deficit rather than a
correctness defect. Amend the observable behavior to require that **the caller's identifier survives
normalization into every exact-match lane key**, and treat verification as blocking rather than
`PENDING`. That single clause is what the current text is missing.

---

### D2 — The recall circuit drops the seed from the delivered candidate set (Membrane)

**Problem.** `blueprint/src/graph/recall-circuit.mjs:211-216` builds the candidate set from
`path.nodes.at(-1)` — path *terminals* only. When the seed is the answer (which is the normal case
for "where is X defined"), it appears only if some path happens to terminate on it.

**Measurement.** With D1 fixed, the correct symbol is discovered as a seed but still absent from the
delivered top 10 in **11.6%** of cases on `blueprint/src` and **8.4%** on `coderight-tauri/src`.
Emitting seeds alongside terminals recovers +2.4 pts on `blueprint/src` (68.3% → 70.7%) and nothing
measurable on `coderight-tauri/src`, where the residual loss is the top-10 cut rather than the
terminal-only rule.

**Mechanism.** Emit `path.nodes[0]` alongside the terminal, deduplicated by node id, preserving the
existing `comparePaths` order. Owner: Membrane / Blueprint.

**Note.** This is smaller than D1 and depends on it — with D1 unfixed the seed is wrong anyway, so
delivering it does not help. Fix in the same change; do not schedule it separately.

---

### D3 — Wrong seeds are reported as a complete result (Membrane)

**Problem.** `recall-circuit.mjs:159` sets `state: "complete"` unconditionally once any seed
resolves. In the `assertPublicationCandidate` trace above, five unrelated symbols named `candidate`
produced `state=complete`, zero omissions, and a receipt a consumer would reasonably trust.

**Why it matters for this assessment.** This is precisely the failure mode Section 5 of the brief
warns about: receipts explain rejected candidates, and cannot reveal evidence that acquisition never
found. Neither the Pull receipt nor the Blueprint envelope contains anything that would have
surfaced D1. Membrane's whole value proposition rests on receipts being trustworthy accounts of what
was and was not delivered.

**Mechanism.** Record seed *provenance strength* in the circuit envelope: which lane resolved, and
whether the caller's literal token matched any lane. A result whose seeds came only from fragment
matches should carry an omission (`seed_token_not_matched`) rather than reporting `complete`. This
is an addition to the existing omission vocabulary, not a new authority. Owner: Membrane / Blueprint.

**Benefit.** It makes D1-class regressions detectable from receipts, which is what would have caught
this without an external probe.

---

### D4 — CodeRight ships a second repository map that nothing reads (CodeRight)

**Problem.** `engine/bins/coderight/src/blueprint_init.rs` is spawned on every root session
(`api_sessions/lifecycle.rs:679`). It recursively walks the entire workspace with a seven-name deny
list (`.git`, `target`, `node_modules`, `dist`, `build`, `.next`, `.cache` — no gitignore
awareness), builds a `RepoMap` of per-top-level-directory file counts, byte totals and extension
histograms, and writes an OKF bundle into `.coderight/understanding/` **inside the user's
repository**.

Nothing reads it back. `parse_okf_bundle` is re-exported at `engine/crates/config/src/lib.rs:27` and
has zero call sites in either repository. The only downstream consumer of its existence is
`apps/coderight-tauri/src/lib/transcriptMessages.ts:23-24`, which *filters its announcement out of
the transcript*.

**Why this is a defect and not merely dead code.** It is a full-tree filesystem walk and a write
into the user's working directory, on every session open, on repositories of arbitrary size, with an
ignore policy that will happily descend into `.venv`, `vendor`, `Pods`, `.tox` and build outputs.
The `MAX_MAPPED_FILES = 2_000` cap bounds only the fingerprint, not the walk. And it is a
CodeRight-owned artifact named "blueprint" that produces a "RepoMap" — the exact thing the master
canon's *Explicit relocation: repository context intelligence* section, `MBR-006` and `SRCH-003`
forbid CodeRight from owning.

**Mechanism.** Delete `blueprint_init.rs`, its spawn site, and the desktop transcript filter that
exists only to hide it. This is pure subtraction: it removes work, removes a canon violation,
removes an unexpected write into user repositories, and removes a name collision that makes the real
Blueprint harder to reason about. Owner: CodeRight. Effort: under an hour. Risk: none identified —
there is no consumer to break.

---

### D5 — Hub-off is a total outage for CodeRight, and the contract that fixes it already exists

**Problem.** `main_runtime_session_host.rs:284-291`: if the Membrane binding fails, InlineHost
returns `"canonical Membrane binding required for inline agent execution"` and no session starts.
The binding is a single loopback HTTP `/health` handshake
(`coderight-memory-backend/src/native.rs:440-453`); when the Hub is not running it fails, and
`membrane_client::ensure_action` (`binding.rs:105`) maps `OfflineKnown`/`TimeoutKnown` to
`Refuse`. The team's own receipt records the outcome: CLI probe `40b9073f`, Hub OFF,
`membrane_timeout_known` at `127.0.0.1:47851/health`, **exit 2, zero provider calls, zero tool
calls** (master canon, "2026-09-08 Windows SRCH-002 continuation closure").

**This contradicts Membrane's own canon.** `docs/architecture/execution-lifecycle-boundary.md` is
explicit: explicit operations across all six subsystems remain available with no resident holder;
*"ordinary explicit requests must not fail solely with `hub_inactive`"*; and *"Membrane is
harness-agnostic… Native integration changes transport, not subsystem scope, authority, durable
state, or Hub-off availability."*

**Why it is still broken.** Membrane landed the fix on 2026-09-08 at 03:29 in `3f5dbb8`
(`feat(client): bind explicit operations to installed owner without Hub`): a complete bounded
process transport — `membrane-client/src/explicit.rs` (`InstalledExplicitClient`),
`membrane-protocol/src/explicit.rs` (`ExplicitOperation`, including `Federate`), the server side in
`membrane-runtime/src/explicit_client.rs`, and argv wiring at `membrane-runtime/src/cli.rs:4041`
(`membrane cli explicit-call`). CodeRight pins `membrane-client` at
`e09c23dff19dd099803c79bed6ced6f2d2b77f2e` (`engine/Cargo.toml:162`) — **ten commits behind, and
before `3f5dbb8`.** CodeRight cannot see the type.

**Two changes are needed, one on each side.**

*Membrane:* `ensure_action` still says `Refuse` for a known-offline candidate, and no public helper
tells a consumer that such a candidate is explicit-eligible. `InstalledExplicitClient::connect`
takes exactly the `KnownCandidate` that `DiscoveryOutcome::OfflineKnown { candidate }` carries, and
`locate_installed_candidate` (`binding.rs:309`) is pure filesystem — it resolves the installed root
and executable *without contacting the Hub*. Everything needed is in hand; the public contract just
does not express the transition. Add an explicit-eligibility outcome (or an `explicit_action()`
peer to `ensure_action`) so a consumer following the public API arrives at the bounded lane instead
of refusing. Without this, every native consumer must reach into the enum by hand — which is exactly
how CodeRight ended up HTTP-only.

*CodeRight:* bump the pin, add `membrane-protocol` as a dependency (it is not currently a dependency
at all, so `ExplicitOperation` is unreachable), and add the bounded process transport as a second
lane in `native.rs` — the host supplies the process effect, per the `ExplicitOwnerTransport`
contract's explicit instruction that "hosts inject process effects", including deadline-bounded
termination and tree reaping under the existing `ResourceGovernor`.

**Cost, honestly.** The bounded lane spawns `membrane[.exe] cli explicit-call` per call, and for
`/federate` that process in turn spawns the packaged Node Blueprint one-shot
(`membrane-runtime/src/blueprint_one_shot.rs:139-167`). Two process starts and a cold store open per
call. That is materially slower than the resident path and should be presented as degraded-mode
operation, not as a peer. **But a slow turn is strictly better than no turn**, and the current
behavior — refusing to start at all, for a subsystem whose only actual use is five memory strings —
is the worst available trade.

**Experiment.** With an installed Membrane and Hub stopped: run one `coderight run` one-shot. Accept
if the turn completes with a receipt naming the bounded lane and no CodeRight child process
survives. Reject if a Cortex store is opened by CodeRight, if the lane is selected while a resident
holder is available, or if an uncertain dispatch is replayed. **Result unknown** — I could not
execute an installed Windows binary here.

---

### D6 — Per-message memory injection costs 26 loopback round-trips for five strings (CodeRight)

**Problem.** `native.rs:483-497` re-issues a full `/health` handshake before *every* non-handshake
operation, and the federation transport at `:525` does the same. `retrieve_workspace_memory`
(`api_session_context_runtime.rs:134-222`) then calls `scopes`, `recall_scored` and
`record_injections` (3 operations = 6 round-trips), and calls
`MemoryProvenanceStore::effective_for` — which is a `get_full` backend read
(`memory_provenance.rs:169-180`) — **twice per admitted item**: once in the rendering loop and again
in the receipt loop. At `limit = 5` that is 20 more round-trips. **26 loopback HTTP round-trips per
user message to inject five memory strings**, of which 13 are redundant handshakes and 10 are
duplicate provenance reads.

I report round-trip counts, which follow from the code path, and deliberately **not** milliseconds —
I did not run the binary.

**Mechanism.** Two independent, purely local fixes, neither touching authority: resolve provenance
once per item and reuse it for both the rendered block and the receipt; and fence the identity
handshake to a generation/short-TTL window rather than per call — the fence exists to detect
installation rotation, which does not change between two operations microseconds apart. Owner:
CodeRight.

---

## 5. Design gaps (strongly justified, not defects)

### G1 — CodeRight receives one-sixth of Membrane and pays for all of it

**The reasoning.** `MBR-001` makes a compatible Membrane binding mandatory for every normal root
turn. That gate is justified only if Membrane is supplying the context plane. It is not: the trait
is `MemoryBackend`, the automatic context is `recall_scored`, and the only Membrane call on a turn
is Cortex memory. CodeRight therefore pays Membrane's full availability cost (D5: total outage) for
Cortex's value alone, while Blueprint — the subsystem that would actually change task outcomes — is
absent from the turn path, absent from the tool registry
(`engine/crates/tools/src/registry/registry_impl.rs` contains no Membrane tool), and absent from any
default MCP registration.

`MBR-005` — "every normal root-model dispatch computes bounded host capacity, calls canonical
Membrane and consumes its context contract before provider serialization" — is the atom that closes
this, and it is correctly marked Pending. But its only implementation, `context_federation.rs`, is
wired to a ServiceHost HTTP route (`api_membrane_background.rs:38`) whose sole caller repository-wide
is `api_tests_part_37.rs:42`, and the router is built only in `main_runtime_server.rs:872`.
InlineHost constructs the client (`main_runtime_session_host.rs:284`), stores it, and has no way to
call it. Since `KRN-001/003` closed and InlineHost became the default, **`MBR-005`'s implementation
became unreachable in the default host** — the two closures interact in a way neither atom
anticipated.

**Cheapest credible step, and it is not `MBR-005`.** Canon §9 of `docs/canon/README.md` already
permits "a thin explicit pass-through where a host surface requires it". Expose `blueprint recall`
as an explicit CodeRight tool over the existing Membrane binding. That is a tool registration and a
call, not a context authority; it gives the model the capability its own agent rules already assume
exists; and it is testable immediately. It does not substitute for `MBR-005` — automatic
pre-dispatch context admission is still Membrane-owned and still needed — but it delivers most of
the user-visible value at a fraction of the cost, and it makes `MBR-005`'s eventual benefit
measurable against a real baseline instead of against nothing.

**Order matters.** Doing this before D1 would wire the agent directly to a retriever that misses
83% of camelCase symbol lookups. Fix D1 first.

**Second-order gap worth naming.** Retrieval runs *once*, at message admission, keyed on the raw
user sentence, with a hardcoded `limit = 5`, and never again as the task evolves. On a twenty-step
agentic turn, the agent discovers the relevant symbol at step three and no evidence acquisition ever
uses that discovery. `HCTX-002`'s safe provider-turn boundaries are exactly the seam where a
re-query would belong. This is the composition the architecture cannot currently represent: evidence
acquisition is bound to *message arrival* rather than to *decision points*.

### G2 — The observation loop is specified on both sides and connected on neither

`ingest_coderight_snapshot` (`Membrane/engine/crates/membrane-runtime/src/host_observation_ingress.rs:514`)
has **zero callers** anywhere. CodeRight has its own structurally parallel `EvaluationOutcomeV1`
(`engine/crates/telemetry/src/evidence.rs:407`) that is never sent to Membrane.
`docs/pending/CODERIGHT-MEMBRANE-INTEGRATION.md` §0 records Streams A–D (transcript events,
selected-transcript evidence, execution observations, evaluation outcomes) as `NOT STARTED` — that
ledger is otherwise stale, but on this point it is still accurate.

**Consequence, stated causally.** Adapt cannot propose improvements without observations, and Cortex
cannot learn which memories actually helped without outcomes. The only learning in the product today
is an agent choosing to call `memory_save`. So "does Cortex/Adapt genuinely improve later tasks?"
has a clean answer for CodeRight users: **Adapt cannot, because it receives nothing; Cortex can only
to the extent the agent remembered to write something down.** That is not a defect in Adapt — it is
a missing wire, and it is the wire that would let either system measure itself.

I do **not** recommend building this next. It is the right thing to build *after* retrieval works,
because otherwise the first thing the loop learns is the noise from D1.

---

## 6. Measurement gaps that affect what you can conclude

Blueprint's checked-in retrieval evidence is `evals/retrieval-corpus/corpus.v1.json`: **30 cases
across four fixture repositories totalling 18 files**. Three problems compound:

1. **It measures the wrong function.** `scripts/benchmark-retrieval.mjs:37,68` exercises
   `queryGraph`, not the production `executeRecallCircuit`/`recallCircuitToCandidateSet`. I measured
   both: `queryGraph` gets 99.4% on exact identifiers; the production path gets 32.9%. The
   checked-in report (`graph`/`hybrid` at 86.7%) therefore describes behavior no production consumer
   uses. Re-running it at current main gives 83.3% — a small drift that also went unnoticed.
2. **It measures the wrong provider.** The harness builds with `buildGraphGeneration` only. The
   shipped selected provider is tree-sitter (`static-provider.mjs:296,320-326`,
   `manifest.provider = TS_PROVIDER`). So the corpus scores the lexical *fallback*. I ran the
   comparison: on this corpus, tree-sitter and lexical produce **identical** results — 83.3% hit
   rate, 0.761 MRR, and **zero per-case differences**. That does not show tree-sitter is worthless;
   it shows the corpus cannot detect the difference between the selected provider and its fallback.
   The in-source justification for selecting tree-sitter ("cleared every qualification gate the
   lexical incumbent has, 12/12 tasks, 6/6 gates") is a claim of *parity* on a corpus too small to
   discriminate.
3. **It is too small to see scale effects.** Every effect I measured got worse as the graph grew
   (32.9% → 17.1% from 4.7k to 10k nodes). Eighteen files cannot show that.

Related: `Membrane/docs/pending/README.md` reports **lifecycle closure-proven: 0 of 340** committed
capability atoms. Combined with CodeRight's 48 pending atoms, both systems carry a large
declared-but-unproven surface. That is a reason to prefer changes whose benefit can be *measured*
now over changes that add to the unproven pile.

**Recommendation.** Point the existing harness at the production recall path and at a
tree-sitter-augmented build, and add one real repository (a few thousand symbols) with
source-derived ground truth. My probe scripts are committed alongside this report and do exactly
this; they are diagnostics, not a replacement for the owned harness.

---

## 7. Speculative — worth an experiment, not a commitment

**S1 — Natural-language retrieval.** Measured NL top-10: **8.5%** (`blueprint/src`), **6.6%**
(`coderight-tauri/src`), and the D1 fix does not move it (21.3% → 22.0% at the circuit level). Every
seed lane is exact string or token equality — no stemming, no fuzzy matching, no embedding over the
code graph. Cortex has an embedder (`embedder_dim`), but it serves durable memory, not the graph.
Real coding requests arrive as prose, so this is a real gap.

I am deliberately **not** recommending semantic retrieval yet, for a specific reason: with D1
unfixed you would be comparing an embedding layer against a broken exact lane, and would very likely
conclude that embeddings are the fix when the exact lane was simply never working. Fix D1, re-measure
NL, *then* run the experiment. `BPT-D016` already holds personalized graph diffusion as a measured
experiment with no default activation — the same discipline applies here.

**Specified experiment.** Add a stemmed/fuzzy fallback lane below `bounded_lexical`, firing only
when the exact lanes abstain, with its own `seedExactness` rank so `comparePaths` keeps it strictly
below exact evidence. Accept if NL top-10 improves materially on two real repositories with no
regression in exact-identifier recall, no rise in `junk_on_no_gold`, and no change to the
non-compensatory ordering contract. **Result unknown.**

---

## 8. Decision

### The three changes I would make first

1. **Fix Blueprint seed resolution (D1 + D2 + D3), together, in Membrane.** Largest measured effect
   in either system (17.1% → 59.2% on CodeRight's own desktop app; 32.9% → 70.7% on Blueprint's own source), smallest change, no new
   authority, no architecture movement, and it sits under the one call the agent rules mandate
   first. Ship D3's seed-provenance omission in the same change so the next regression of this class
   is visible in a receipt instead of requiring an external probe.
2. **Delete `blueprint_init.rs` in CodeRight (D4).** Pure subtraction. Removes a per-session
   full-tree walk, an unexpected write into user repositories, a canon violation, and a name
   collision with the real Blueprint. Nothing consumes it.
3. **Restore Hub-off operation (D5), on both sides.** Membrane: express explicit-eligibility in the
   public discovery contract so a consumer following `ensure_action` reaches the bounded lane.
   CodeRight: bump the `membrane-client` pin past `3f5dbb8`, add `membrane-protocol`, and add the
   bounded process lane. This removes a total-outage failure mode and closes the gap between
   Membrane's stated harness-agnostic contract and its actual native-consumer behavior.

**Dependency order.** D1+D2+D3 first — everything downstream amplifies whatever retrieval does, so
retrieval must be right before more of it is wired in. D4 is independent and can land any time; do
it first if you want a quick clean-up. D5 is independent of D1 and can proceed in parallel, but its
CodeRight half must not land before the pin bump. G1's thin `blueprint recall` pass-through comes
*after* D1, not before. D6 is a local optimization to fold into whichever CodeRight change lands
first. G2 (the observation loop) comes after retrieval works, so the loop learns from a system worth
learning from.

### What should not change

The InlineHost/ServiceHost topology and the `KRN-*` closures — the evidence is real and the design is
sound. The effect/approval pipeline — `EFF-002` is genuinely closed at source and needs evidence, not
redesign. SQLite as CodeRight's durable event authority. The CodeRight-execution / Membrane-context
ownership seam — every problem I found is a wiring or algorithm problem *inside* the correct
boundary, and none of them argues for moving it. Pull's federation, admission and receipt model —
well-built and simply not reached. Membrane's non-compensatory `comparePaths` ordering — the D1 fix
must slot under it, not replace it.

### Rejected or deferred, with reasons

- **A CodeRight-side ranker or context planner.** Rejected. Canon forbids it, and the evidence points
  the other way: the defect is in Membrane's own exact lane, and a CodeRight reranker would sit
  downstream of a retriever that never returned the right candidate.
- **Reopening the host topology, or a `harness/*` rewrite.** Rejected. Nothing I found implicates it.
- **Rewriting Pull or the federation client.** Rejected. It is well-designed; it is unreached.
- **Semantic/embedding retrieval now (S1).** Deferred, with a specified experiment. Running it before
  D1 would produce a confidently wrong conclusion.
- **Growing the eval corpus as the first move.** Deferred. It is genuinely needed (Section 6), but
  D1's effect is large enough and cross-validated on two real repositories, so acting on it should
  not wait for corpus work.
- **Closing `MBR-005` before the thin pass-through.** Deferred. Same effect, much larger change,
  and no baseline to measure it against until the agent has Blueprint at all.

### Evidence gaps that could change this ranking

- **Language coverage.** I measured TypeScript and JavaScript (severe), Swift (material) and Rust
  (mild). Python, Java, C#, Go and Kotlin are untested; Python should behave like Rust and the rest
  like Swift or worse, but that is inference, not measurement. If your users are overwhelmingly
  writing snake_case languages, D1 drops in priority — though it would still be the cheapest fix
  available in either system.
- **Does the D1 gain survive the full pipeline?** I measured Blueprint recall in isolation. Pull
  admission, budgeting and packet reduction sit downstream; if they re-filter on something
  correlated with the current (broken) seed distribution, the delivered gain could be smaller.
  Measuring `/federate` end-to-end with a real installed Membrane would settle it.
- **Task-level outcomes.** Every number here is retrieval containment against source-derived ground
  truth. None of it measures whether a task completed correctly, how many tokens a turn used, or how
  long anything took. Those require running the real product against real tasks, which I could not do.
- **D5's real cost.** Two process spawns per bounded call is a code-path fact; its actual latency is
  not. If it proves badly slow, the right answer may be a bounded resident helper rather than
  per-call spawning — which would be a Membrane design decision, not a CodeRight workaround.
- **`membrane_context` vs `/federate`.** `ExplicitOperation` exposes `Federate` but not `Context` or
  `SourceRead`, so the Hub-off lane serves CodeRight's legacy `/federate` route and not the two
  canonical operations `MembraneClient` exposes. `MBR-005`'s instruction to converge `/federate` onto
  `membrane_context` and the explicit lane's operation set need to be reconciled before both land.

---

## Appendix — reproducing the measurements

Diagnostic probes are committed beside this report under `probes/`. They are isolated read-only
diagnostics: they copy a source tree to a temp directory, build a graph there, and query it. They
write nothing into either repository and are not a replacement for Blueprint's owned harness.

```bash
cd Membrane/blueprint && pnpm install --ignore-scripts   # node:sqlite + tree-sitter wasms

# D1/D2 A/B: current seed resolver vs the two-line variant, any repo
node <report-dir>/probes/recall-ab.mjs <path-to-source-tree>

# D1 stage attribution: discovered-as-seed vs delivered-in-candidates
node <report-dir>/probes/recall-stage.mjs <path-to-source-tree>

# Section 6: shipped tree-sitter provider vs lexical fallback on the 30-case corpus
node <report-dir>/probes/provider-ab.mjs
```

`probes/fixed/` holds the two patched modules (`seed-resolver.mjs`, `recall-circuit.mjs`) that the
A/B arm loads. The only differences from the originals are: `terms()` also emits the whole query
token; the `qualified_symbol` and `exact_term` lanes compare with `lower(...)`; and
`recallCircuitToCandidateSet` emits `path.nodes[0]` alongside the terminal. Import paths are
absolute so the patched copies reuse every unmodified upstream module.

Ground truth in every probe is derived by regex over the source files themselves — exported
functions and their definition sites — never from the retriever whose recall is being measured.
