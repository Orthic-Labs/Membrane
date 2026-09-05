# Membrane Push: final implementation plan and canon revision

## Post-merge implementation status — 5 September 2026

This document was authored as the pre-implementation handoff at audited revision `75c257ad711d19ffce69258d132a45dbffa9b4ac`; statements below describing what was missing are therefore historical source findings at that revision. The repair implementation was subsequently merged through PR #11 at merge commit `cb0cbbcc308f345f8d6c063eb458040f1e37f8c8` from implementation head `09694be7f457bdc6ea1eff07254afd8c8db7d23f`.

The governing inventory remains **29 committed Push capabilities (PSH-001–PSH-029)**. The merged implementation adds the shared recovery store/resolver, exact selectors, consumer-bound recovery proof, measured packet/final-wire selection, native MCP and resident routes, owned JavaScript tool egress, bounded capture/storage, and explicit lifetime semantics described by this plan. The companion canon records these mechanisms conservatively as implementation progress; it does **not** promote release qualification, installed-host coverage, provider-billed savings, or held-out quality evidence.

Compile-level validation is already available for the merged implementation: the focused Push workflow at implementation head completed its Cargo check, and the PR Windows lane also completed Rust compilation before later compatibility tests failed. Per current repository coordination, this documentation follow-up intentionally does not run the full CI suite while other subsystems are changing.

Post-merge implementation receipt: `docs/provenance/foundation/2026-09-05-push-post-merge-implementation/verification.md`.

---

**Audit date:** 5 September 2026
**Repository:** `Orthic-Labs/Membrane` — the supplied `orthiclabs/membrane` URL did not resolve; the working repository is the hyphenated organization.
**Audited revision:** `75c257ad711d19ffce69258d132a45dbffa9b4ac`
**Method:** pinned-source review, caller/consumer tracing, canon reconciliation, and primary-source comparison with five closely related projects.
**Specification revision:** `push-final-2026-09-05-v1`
**Repository head recheck:** `main` still resolved to the audited revision during consolidation.
**Companion canon:** `docs/canon/push.md` (supplied separately as `Membrane_Push_Canon_Revised.md`).
**Scope:** 24 retained atom IDs plus five introduced requirements, PSH-025–PSH-029: **29 committed Push capabilities** in the supplied revision.
**Change status:** final proposed implementation specification and revised capability ledger; no repository branch, installed binary, setting or remote file was changed. No implementation or release qualification is claimed.

> **Verdict: Push is a real implementation, and native MCP can reach its packet-reduction path. It is not yet a consistently wired, safely recoverable, general-purpose tool-output layer. Fix the delivery and recovery contracts before expanding the compressor catalogue.**

The distinction matters. “There is a Push module,” “an agent can invoke one path,” “every relevant tool output passes through it,” and “every reduction can be safely recovered” are four different claims. The source supports the first, conditionally supports the second, and does not establish the last two.

## Reading this revision

This document consolidates the earlier `Membrane_Push_Audit_and_Improvement_Plan.md`, the submitted `membrane-push.md`, and the follow-up source corrections. It supersedes both as the implementation handoff. The companion atomic canon retains the repository's five-register structure rather than replacing the atom ledger with architectural prose.

**Source-derived observations** remain pinned to the audited commit. **Design decisions and acceptance criteria** below describe work to implement, not behavior already present. **Historical focused-test states** are preserved only where explicitly identified; this consolidation did not execute those tests.

The supplemental document contributes an explicit preservation validator, bounded segment-decision reporting, shared language-extraction guidance, and visible anchor expiry. Its claims of universal egress coverage, correct expiry on every route, a missing LLMLingua engine, a proof-grade donor validator, LLMLingua-based artifact recovery, and an H8-times-turns session budget are not adopted. See sections 5 and 6.8 for the reconciliation.

The installation bundle also contains the source-comparison receipt, introduction authority, and preservation-register additions required by the existing canon checker. Replacing only the canon file is not sufficient to integrate new atom IDs into that checker.

## 1. Executive findings

The native route is substantially further along than a review of the JavaScript MCP files alone would suggest. The native tool schema requires `remainingContextCeiling`; `RuntimeMcpExecutor` forwards it; native federation invokes `select_packet_for_h8_with_policy` and publishes the selected packet. **Do not rebuild that integration from scratch or report that native H8 forwarding is missing.** [Native schema][M12] · [Native executor][M11] · [Federation selection][M17-selection]

The most important remaining defects are:

| Priority | Finding | Practical consequence |
|---|---|---|
| P0 | Representation selection uses planned totals rather than a verified measurement of the materialized, final delivery. Its local estimator is whitespace-based. | A representation can be reported as fitting even when it does not; a host estimator label must not be mistaken for a measurement performed with that estimator. |
| P0 | HTTP and CLI recovery do not share mandatory digest/expiry verification. | An anchor is not currently a reliable integrity-and-lifetime contract across transports. |
| P0 | CLI and service recovery roots differ by default; small `runc` results still advertise an anchor after their capture has been deleted. | An agent can receive a reference that the intended resolver cannot find. |
| P0 | The CLI `Runc` path joins arguments into a shell command instead of using the existing validated adapter path. | The safety helpers are not the safety boundary of the normal CLI caller. |
| P0 | Query-aware opt-in creates `authority_admitted=true` and `freshness_valid=true`; a code fallback can compress again after a query-aware refusal. | Request intent and successful gating are not cleanly separated, and refusal is not reliably terminal. |
| P1 | Neither the native nor JavaScript MCP tool registry exposes generic Push recovery. | A generic MCP agent cannot resolve `mr://anchor/...` through a declared Push tool. Document-section reads are not an equivalent substitute. |
| P1 | Retained JavaScript client/hook paths omit H8, although the service requires it. | Those paths cannot satisfy the current contract as written. This is separate from the repaired native path. |
| P1 | Some AST-backed skeleton rendering is actually first-line/string splitting. | Multiline signatures, destructured parameters, and TSX need stronger handling and regression tests. |
| P1 | Native MCP omits the dedicated packet-reduction result and repeats selected blocks under both `packet` and `candidates`. | The consumer loses selection evidence while its output envelope can contain redundant content. |
| P1 | Push observations have untyped `before`/`after` values and inconsistent call-site units. | Current observations are insufficient to establish trustworthy token savings, adoption, or task-quality outcomes. |

These are source-level findings, not claims that a production incident was observed. Sections 3–5 provide evidence, scope qualifications, and fixes for each one.

### The recommended direction

Keep Push as **Membrane’s representation and recoverability layer**. Reuse its existing transforms, protocol types, native selection, and governed storage boundaries. Add a small shared preparation/recovery owner, make the native agent surface complete, and qualify one real host-output integration end to end before adding more.

Do not turn Push into another memory system, another relevance planner, or a remote command-execution server.

## 2. Is it wired in? Can the harness or agent access it?

### 2.1 Actual surface matrix

| Surface | What is present at the audited revision | Access verdict |
|---|---|---|
| Native `membrane_context` MCP tool | Advertises H8; executor forwards it; native federation runs the reduction selector. | **Yes, conditionally.** Requires a valid host observation with the documented session/task bindings, an active Hub, and normal admission prerequisites. |
| Native MCP stdio and HTTP | Stdio uses the Hub transport; the resident Hub installs the native executor and merges the native MCP HTTP router. | **A real native transport exists.** This is not just a JavaScript prototype. |
| Direct resident `POST /federate` | Dispatches directly to native federation and its H8 requirement. | **Yes, for a correctly formed request.** No missing-H8 enrichment occurs in the intervening wrapper. |
| Retained `mcp/client.mjs` CLI input path | `loadInput` whitelists several envelopes but not `remainingContextCeiling`. | **Contract mismatch.** Supplying H8 in the input file does not preserve it on this path. |
| Retained `mcp/host/context-adapter.cjs` resident path | Constructs a small request containing task/repository/budget/client/session/anchors, without H8. | **Contract mismatch.** It also collapses a rejected resident call into an unavailable/degraded result. Whether this is the installed hook on a particular machine was not observed. |
| Push CLI branches | `Skel`, `Compress`, `Prep`, `Select`, `Runc`, and `Restore` have implementations. | **Manual access exists** for an agent permitted to run the installed CLI. This does not demonstrate automatic interception. |
| Resident `POST /compress` | Calls the prose compressor and returns output. | **A transformation endpoint exists**, but not a complete raw-first, recoverable egress contract. |
| Resident `POST /expand` | Resolves a strictly parsed anchor within its configured directory. | **Endpoint exists, but verification/lifetime/boundedness need repair.** |
| Native/JavaScript MCP generic Push recovery | No recovery tool in either registry; native resources are installation/lease/operation metadata resources, not anchor-content resolution. | **Missing as a discoverable agent capability.** |
| `membrane_source_read` | Reads a governed document section with source/anchor/hash inputs. | **Useful but different.** It does not resolve arbitrary historical `runc` output, stdin, or general Push artifacts. |
| Automatic reduction of arbitrary host-tool or third-party MCP output | The canon records incomplete egress coverage; inspected adapters do not establish a universal output-rewrite boundary. | **Not qualified.** A Membrane MCP server cannot be assumed to intercept another tool’s results merely because both are installed. |

Evidence: [native startup/transport][M10-native], [native executor][M11], [native registry][M12], [native resources][M13], [service wrapper][M10-federate], [JavaScript client][M14-input], [host adapter][M15], [CLI][M05], [HTTP compression][M10-compress], [MCP toolsets][M16-tools], and [current canon][M01].

### 2.2 Current native data flow

```text
Host / agent
  -> native MCP membrane_context
  -> RuntimeMcpExecutor
       validates caller / authorization
       forwards remainingContextCeiling
  -> native_route_response
       validates H8
       runs native federation and admission
       builds the context packet
       builds full / reduced_1 / floor representations
       selects a representation
  -> RuntimeMcpExecutor shapes the MCP result
  -> host decides what enters model context
```

The reduction call is real. The outstanding questions are whether the host observation is genuinely host-produced, whether the measured representation fits, whether recovery is reachable, and whether the final MCP/model rendering preserves the intended content and evidence. [Native executor][M11] · [Federation][M17-selection] · [Selection implementation][M06]

### 2.3 The missing general-output flow

The target is not “teach the model to remember to call `compress`.” For an owned harness, it should be:

```text
Already-authorized tool execution, performed once by its existing owner
  -> immutable original or verified original reference
  -> typed Push preparation request
  -> eligible representation candidates
  -> final-wire measurement and selection
  -> compact tool result + content-free receipt + usable recovery reference
  -> model

Model asks for omitted evidence
  -> declared recovery operation
  -> same scope/store/expiry/integrity checks
  -> bounded exact slice or exact original
  -> terminal exact lane: do not compress it again
```

For third-party hosts, this requires an actually supported hook, extension, SDK wrapper, or explicitly configured request boundary. An advisory instruction is not equivalent to enforced interception. Keep those capabilities separate in diagnostics.

## 3. Confirmed correctness and safety defects

The audit IDs below are local report identifiers, **not proposed replacements for canonical `PSH-*` IDs**. P0 means repair before treating the affected route as a trustworthy automatic reduction/recovery boundary. It does not mean every finding is an externally exploitable vulnerability.

### AUD-PUSH-01 — Planned size is not the materialized size

**Priority:** P0
**Canon:** PSH-004, PSH-006, PSH-011, PSH-012, PSH-016, PSH-019

`build_packet_reduction_plan_with_policy` derives representation totals from existing packet metadata and block allocations. `representation_content` then performs the transformations and rewrites the packet’s admitted total to the planned number. Individual transformed blocks may update their token estimates, but that does not establish the representation’s actual final cost. Structured blocks are copied unchanged, and protected-overflow/fallback behavior can return more material than the allocation allowed. [Plan construction][M06] · [Materialization and fallback][M06-transform]

There is a second, independent problem: `compress::estimate_tokens` counts whitespace-separated spans. That is a lexical estimate, not a model tokenizer. Copying the H8 estimator basis into the representation plan does not cause the transformation to have been counted with that basis. Dense JSON, code, identifiers, and text without spaces are especially important test classes. [Estimator and budget result][M07-budget] · [Selection][M06]

**Source-level counterexample to test, not a recorded production trace:** construct an otherwise valid packet containing a protected 100-unit block and a structured 1,000-unit block, with the latter allotted 200 units. Its full total is 1,100; its planned reduced total is 300. If structured content is copied, materialization remains 1,100 even though the plan advertises 300. A ceiling of 350 must not admit that representation. This illustrates the missing invariant even before accounting for JSON wrappers, markers, or MCP rendering.

**Required change:** materialize each candidate, validate its protected content and recovery requirements, serialize the actual delivery form, measure it with the declared estimator, and only then select. Propagate `budget_met=false` rather than discarding it with `.text`. A fallback is another candidate to measure, not an exemption from the ceiling.

The contract should distinguish:

- exact model-token measurements, including tokenizer identity/version;
- explicitly named conservative estimates with their own uncertainty;
- bytes and characters, which must never be silently relabeled as tokens.

A fitting candidate must satisfy the bound for its final delivered representation. If protected content alone cannot fit, emit a typed capacity refusal and let the planner/host replan. Do not silently remove protected evidence or invent a larger capacity.

**Acceptance:** structured pass-through, protected overflow, unsupported code, unknown tokenizer, dense JSON, non-space-delimited text, and wrapper overhead all have regression fixtures. The receipt records the measured candidate and actual estimator, not just its original allocation.

### AUD-PUSH-02 — Recovery verification is not mandatory across transports

**Priority:** P0
**Canon:** PSH-002, PSH-005, PSH-020, PSH-022, PSH-023, PSH-024

The repository already contains `verify_recovery_marker`, which checks schema, expiry, and the recovered bytes’ source digest. The problem is not the absence of a helper; it is that the inspected resolvers do not use a shared mandatory verification boundary. [Verifier][M07-verify]

`expand_anchor_response` parses and confines the anchor, reads the file, computes a new hash, and returns it. It does not compare that hash with the expected anchor/source digest before returning content. A newly calculated digest is not verification against an expected identity. Expiry rejection occurs only if the sidecar can be read and parsed and exposes the expected expiry value. Missing or malformed metadata therefore does not fail closed. [HTTP expansion][M10-expand]

The CLI restore branch confines the path but prints the file without checking the recovery marker’s schema, source digest, or expiry. Both CLI and HTTP paths read the entire artifact as UTF-8 text; that is not a bounded, binary-safe exact-byte retrieval contract. [CLI restore][M05-restore]

**Required change:** route CLI, HTTP, native MCP, and in-process recovery through one `RecoveryStore::resolve_verified` boundary. It must verify identity, authorization, lifetime, metadata version, and the original bytes before exposing a slice. Missing/corrupt metadata must be a typed failure, or an explicitly versioned legacy migration path—not implicit permission to serve forever.

Retain the existing strict anchor grammar and confinement. Add repository/session/grant scoping where the product’s ownership model requires it; an installation bearer token is not automatically a per-artifact authorization decision. Use byte-safe transport or explicitly reject unsupported encodings without claiming exact restoration.

**Acceptance:** altered bytes under a valid-looking filename are rejected; missing/malformed sidecars do not bypass lifetime rules; expired CLI and HTTP reads behave consistently; cross-scope access is denied; large restores are bounded; binary and CRLF fixtures round-trip under the declared fidelity contract.

### AUD-PUSH-03 — Store identity and publication do not guarantee resolvable handles

**Priority:** P0
**Canon:** PSH-001, PSH-002, PSH-005, PSH-006, PSH-020, PSH-023

There are three separate problems with the same underlying ownership boundary.

**Different default roots.** CLI `Runc` defaults to `<current-directory>/.cache/runc`; the resident service defaults to `<configured-workspace>/tools/.cache/runc`, unless its anchor directory is overridden. Even with the same workspace/current directory, these are different paths. A common anchor spelling does not provide a common backing store. [CLI defaults][M05-restore] · [Service defaults][M10-roots]

**A handle can outlive no stored object at all.** For small command output, `run_command_capped` reads the capture and deletes it instead of publishing a spill. It still returns an anchor derived from the content digest, and the CLI prints `[anchor]` unconditionally. A digest can be useful as a content identity, but it must not be advertised as a dereferenceable recovery reference when there is no retained object. [Capture completion][M09-run] · [CLI rendering][M05-restore]

**Publication lacks final-object read-back verification.** `publish_spill` hashes the capture and writes content-addressed output and metadata, but does not verify the final published file before returning a handle. On the existing-target path after a rename error, it can accept an existing file and delete the new capture without verifying that existing file’s content. [Publication][M09-publish]

**Required change:** resolve storage through a shared installation/store identity, not through independent current-directory conventions. Publish atomically; verify the object selected for reuse as well as a newly published one; commit metadata; then issue the handle. Return separate fields for `contentDigest` and `recoveryHandle`, with the latter absent unless actually resolvable.

The same owner must cover lossy stdin/prose and code preparation. The pure compressor and skeletonizer correctly return no recovery marker when they have not published a source. Preserve that honesty, but add an owner above them that retains the original before a lossy result is sold as recoverable. A mutable source path is not automatically a snapshot of the pre-transform bytes. [Pure compression][M07-budget] · [Pure skeletonization][M08-budget]

**Acceptance:** default CLI-to-HTTP-to-MCP recovery uses one store without manual path coordination; restarting the Hub does not strand a live handle; an existing corrupt target cannot be reused; a no-spill result never advertises an unresolvable recovery handle; every lossy stdin result follows the declared retention policy.

### AUD-PUSH-04 — The normal CLI caller bypasses the safer execution adapter

**Priority:** P0
**Canon:** PSH-018, PSH-020, PSH-021

The runtime has a meaningful `run_adapter_capped` path: it validates an adapter against a canonical repository root, launches a program with an argument vector, and sanitizes Git-specific environment state. However, CLI `Runc` joins the supplied argument vector into a string and calls `run_capped`, which constructs a platform-shell command. [Validated adapter and shell path][M09-run] · [CLI dispatch][M05-restore]

Consequently, the existence of allowlist, confinement, and shell-free helpers is not evidence that the standard CLI execution surface uses them. Argument joining also loses the original argument boundaries.

**Required change:** either wire the CLI’s governed execution mode to the validated adapter, or explicitly separate a legacy/approved-shell mode from a governed argv mode. Do not silently change the meaning of existing approved shell scripts. For an owned harness, prefer processing output from its already-authorized execution instead of transferring execution ownership to Push.

The resident service deliberately has no `/runc` endpoint. Keep that boundary. Adding remote shell execution to solve output compression would expand authority unnecessarily. [Service design][M10-header]

**Acceptance:** arguments with spaces, quotes, Unicode, and shell metacharacters retain their intended boundaries; adapter-specific refusals happen before spawning; Git environment handling is tested; exit status, stderr, and cancellation remain observable; recovery/fallback never executes the original command a second time.

### AUD-PUSH-05 — Query-aware intent is conflated with proof, and refusal can be undone

**Priority:** P0
**Canon:** PSH-007, PSH-010, PSH-013, PSH-014

`push_policy_for_request` selects query-aware mode from `pushPolicy: "queryAware"` and constructs the policy with both admission and freshness booleans set to `true`. Native federation does perform other admission/freshness work, so this is **not a demonstrated bypass of every upstream authorization gate**. Nevertheless, these flags are not derived from the relevant validated observations at this call site; an opt-in is being used as a proxy for proof. [Policy construction][M17-policy]

Separately, `query_aware_text` returns the original on refusal or an empty result. In the code branch of `reduce_block_for_push`, an unchanged result triggers a generic compression fallback. Therefore a deliberate “do not reduce” outcome can become “reduce using another path.” The same string-based fallback confuses unsupported/non-reducing skeletonization with permission to use a prose compressor. [Reduction dispatch][M06-transform]

Native MCP does not currently advertise or forward `pushPolicy` in its context request. That means the native tool’s current request path should not be described as exposing an agent-selectable query-aware mode. Do not repair this by allowing the model to author its own `authority_admitted` or freshness claims. [Native schema][M12] · [Native request construction][M11]

**Required change:** represent transform results as typed outcomes such as `Reduced`, `KeptExact`, `NotApplicable`, `Refused`, and `BudgetUnmet`. A refusal must stay terminal across fallback composition. Bind query-aware policy to actual planner authority, source identity, and freshness receipts. Intent may request a mode; it cannot manufacture its eligibility.

**Acceptance:** query-aware refusal on a `.rs` or `.ts` block returns exact source or a typed capacity refusal, never a second lossy reduction; stale or unavailable proof cannot become `true`; unsupported syntax follows an explicit policy; the native default/control path remains unchanged unless an approved producer supplies the new policy contract.

## 4. Agent integration and fidelity defects

### AUD-PUSH-06 — Recovery is not a first-class MCP capability

**Priority:** P1, and a blocker to advertising automatic offloading as agent-recoverable
**Canon:** PSH-002, PSH-008, PSH-009, PSH-024

The native registry lists context, source-read, Blueprint, memory/workflow, feedback, and diagnostic tools. Its negotiated default is `membrane_context`. There is no generic Push resolve/expand tool. The JavaScript toolsets likewise contain no such operation. Native resource discovery does not provide an alternative anchor-content resolver. [Native tools][M12] · [JavaScript tools][M16-tools] · [Native resources][M13]

`membrane_source_read` is a governed worktree-document read with a source reference, section anchor, and expected content hash. It is not a generic resolver for captured command output or a prior version of an arbitrary compressed input. Do not overload it with unrelated `mr://` semantics without a deliberately versioned source abstraction. [Native source read][M11]

**Required change:** add one narrow, read-only recovery operation—provisionally `membrane_push_resolve`—backed by the shared verified resolver. Update the native schema/dispatch, authorization action mapping, negotiated discovery, operation registry, generated host contracts, and agent instructions together. Compatibility JS should bridge to the same operation or be retired, not become an independent implementation.

Before emitting an offload marker, establish that the current consumer can discover and call this operation against the same store and scope. Merely having a resolver somewhere in the process is not enough. If the tool is unavailable, use a complete eligible inline representation or return a typed refusal.

**Acceptance:** use the normal `tools/list` and `tools/call` flow to recover a marker produced in the same session, then repeat after a Hub restart. A resolver-less negotiated toolset must never receive an offload-only result as though it were recoverable.

### AUD-PUSH-07 — Retained JavaScript paths cannot carry the native H8 requirement

**Priority:** P1; operationally blocking wherever these paths remain installed
**Canon:** PSH-008, PSH-011, PSH-012

The host adapter’s resident call rebuilds a body without H8. The standalone JavaScript client’s input loader also excludes it. The `/federate` wrapper passes the request straight to `native_route_response`, which requires H8. These are concrete producer/consumer mismatches, not an inference from a missing search result. [Host adapter][M15] · [Client loader][M14-input] · [Direct service wrapper][M10-federate] · [Native H8 gate][M17-h8]

The native MCP implementation has already repaired the analogous problem and advertises the required field. That positive finding must remain in the canon and audit. Its nested H8 schema still uses generic object descriptions for some protocol types; an agent should not be expected to invent the correct observation shape or its provenance. [Native schema][M12]

**Required change:** identify which launcher/hook files the installed release actually uses. Prefer one generated contract and a thin native bridge. Repair supported JS transports to preserve typed envelopes, or retire their installation references and document the compatibility boundary. Preserve typed refusal details instead of reporting a healthy service as unavailable.

A genuine host must supply capacity observations. Where a generic MCP host cannot observe remaining context, explicitly report that capability gap. Any advisory bounded-retrieval mode must be a separately defined policy decision, not a fabricated H8 value or a silent downgrade of the strict-fit contract.

**Acceptance:** exercise installed—not fixture-only—host configuration. Assert envelope preservation, session/task binding, nonzero/valid provenance, and faithful refusal reporting. Repeat separately for native stdio, native HTTP, and each retained JS transport.

### AUD-PUSH-08 — Tree parsing does not yet guarantee faithful skeletons

**Priority:** P1
**Canon:** PSH-003, PSH-013, PSH-014

The skeletonizer uses Tree-sitter, but some rendering loses the benefit of that parse. Python takes the first line of a definition. JavaScript/TypeScript take the first line and split at the first `{`. `.tsx` is routed to the TypeScript grammar rather than a separate TSX grammar. The inspected path does not reject an error-containing parse before rendering its recognized nodes. [Language selection and renderers][M08-render] · [Dispatch and budget fallback][M08-budget]

Two source-derived regression examples:

```python
# A signature must not stop at its first physical line.
def calculate(
    amount: int,
    tax: float,
) -> float:
    return amount * tax
```

```typescript
// The first opening brace belongs to a parameter, not the function body.
function total({ x, y }: Point): number {
  return x + y;
}
```

The current rendering rules can damage these headers. This conclusion follows from the inspected render functions; these examples were not executed against a built binary in this audit.

**Required change:** slice signatures and declarations using AST fields/source spans rather than first-line heuristics. Preserve the chosen interface contract: decorators, generics, multiline parameters, type annotations, exported declarations, relevant struct fields, enum variants, and method signatures. Explicitly classify which information is intentionally omitted. Support TSX with the correct grammar, or refuse reduction for that shape.

Use parser errors and incomplete parses as a reason to keep exact source, unless a separately qualified partial-parser policy proves the necessary spans. Re-measure fallback output and propagate `budget_met`; a path stub is not a recovery guarantee without a published original.

**Acceptance:** multiline Python, decorators, destructured JS/TS arguments, exported declarations, TSX, nested declarations, Rust public data shapes, incomplete edits, and non-ASCII identifiers are checked for required-span preservation. Include mixed files where one recognized declaration must not cause unrelated important declarations to disappear silently.

### AUD-PUSH-09 — Selection evidence is dropped while content can be duplicated

**Priority:** P1
**Canon:** PSH-011, PSH-016, PSH-019

Native federation attaches `packetReduction` beside the selected packet. The native MCP executor rebuilds a smaller result containing `packet`, `candidates`, ordinary receipts, and status metadata, but not the dedicated reduction result. `candidates` is cloned from `packet.blocks`, so the executor’s result contains the selected block content in two places. The JavaScript `federatePayload` success projection also does not preserve `packetReduction`. [Federation output][M17-selection] · [Native projection][M11] · [JavaScript success projection][M14-output]

The exact cost to the model depends on how the host renders these fields; this audit did not observe a provider request. The source nevertheless establishes two things: dedicated selection evidence is not carried through these projections, and the native result contains redundant block data.

**Required change:** define one model-facing content rendering and a content-free selection receipt containing the selection/ceiling IDs, representation identity, actual count/basis, protected-content verification, recovery references, and typed fallback reason. Do **not** blindly forward a full internal plan containing alternate representation bodies just to preserve evidence. Keep diagnostic detail out of the prompt unless explicitly requested.

Preserve proof identity through native MCP, compatibility transports, and the final host renderer. Make alias/duplicate payload fields metadata-only, or eliminate them through a versioned compatibility change.

**Acceptance:** snapshot the actual rendered MCP/tool result and final request boundary. The selected content is included once in the intended model-facing form; content-free selection evidence remains joinable; unselected/full originals are not accidentally serialized into the model context as diagnostics.

### AUD-PUSH-10 — Adoption and savings observations do not yet form an evidence chain

**Priority:** P1
**Canon:** PSH-016, PSH-017, PSH-019

`PushObservation` has `before` and `after` without a unit or estimator field. CLI calls use character counts, `Runc` can use spill-byte length for the before value, and packet reduction uses lexical token estimates. These numbers cannot safely be aggregated into one token-savings measure. The record also lacks explicit request/turn/tool/selection/recovery correlation fields. [Telemetry schema][M18] · [CLI call sites][M05] · [Selection call sites][M06-transform]

Telemetry is intentionally optional when `MEMBRANE_PUSH_TELEMETRY_PATH` is absent. That failure isolation is appropriate. What is missing is a capability/coverage distinction between “no transform opportunity,” “not observed,” “sink unavailable,” and “observed zero.” Absence of telemetry must not become a claim that Push is unused or working without error. [Telemetry behavior][M18]

**Required change:** version a content-free event schema with explicit byte/character/token measurements and estimator identities. Correlate opportunity → eligibility → execution → selected delivery → restore/failure → externally supplied task outcome. Record passthrough/refusal reasons as well as successful reductions.

Separate immediate wire savings, additional recovery cost, transformation latency, cache effects, and provider-reported usage. Do not estimate a user’s bill from a byte ratio, and do not label an outcome “successful” merely because the reducer ran. Push emits observations; it does not acquire Adapt’s authority to change future policy.

**Acceptance:** unit-consistent aggregation, content-free log checks, event joins, resolver-failure accounting, optional-sink behavior, and denominators are tested. A diagnostics view can distinguish “installed,” “reachable,” “eligible,” “executed,” “delivered,” and “recovered.”

## 5. What the canon already covers—and what actually needs adding

### 5.1 Source baseline and revised inventory

At the audited revision the source canon contains **24 committed capabilities: 7 DELIVERED, 16 PARTIAL and 1 MISSING**, all with release qualification pending. The submitted deep-dive's opening count of five delivered capabilities is not the source-canon count. The source comparison records seven `CURRENT_BEST` and seventeen `CURRENT_INCOMPLETE` entries; those are historical relative-to-corpus classifications, not deployed-host qualification. [Canon][M01] · [Comparison][M03]

The supplied revised canon contains **29 committed capabilities: 3 DELIVERED, 23 PARTIAL and 3 MISSING**, with **29 PENDING qualification rows**. This is a more explicit specification and source-state reconciliation, not completed engineering work.

| Existing atom | Revised disposition | Reason |
|---|---|---|
| PSH-001 | DELIVERED → PARTIAL; verification STALE | Handle publication is not truthful on all capture paths; existing-object reuse lacks final verification. |
| PSH-011 | DELIVERED → PARTIAL; verification STALE | Planned allocation does not prove the materialized/final-rendered representation fits. |
| PSH-019 | DELIVERED → PARTIAL; verification STALE | Inner receipt emission exists, but the strengthened end-to-end receipt contract is not satisfied by MCP projections. |
| PSH-023 | DELIVERED → PARTIAL; verification STALE | HTTP metadata fall-through and CLI expiry omission defeat transport-wide lifetime guarantees. |
| PSH-004, PSH-012, PSH-022 | Retain DELIVERED / FOCUSED_PASS with original evidence | These preserve historical local compression, native capacity-refusal, and strict-anchor-parser claims respectively. They are not newly executed tests or proof of released integrations. |

All other amended rows retain an incomplete implementation disposition and no new successful verification is asserted. New atom implementation states are source-review assessments: missing where no mechanism was identified in the inspected Push paths, partial where some relevant primitives already exist. They are not the result of an exhaustive repository build/test audit.

The master-atom intake already discussed exact/exempt lanes, fidelity classes, final-wire admission, command-aware reducers, structured codecs and repeat handling. This revision makes five selected observable contracts explicit; it does not claim these ideas were all newly discovered. [Prior intake][M04]

### 5.2 Reconcile all 24 existing requirements

These repairs are now reflected in the supplied companion canon. They do not change the remote repository until the revision is applied. The table maps work to the existing IDs rather than creating duplicate atoms.

| Canon ID | Disposition and next action |
|---|---|
| PSH-001 | Retain the capture mechanics, reclassify the complete claim as PARTIAL, verify publication, and stop advertising recovery handles for deleted captures. |
| PSH-002 | Put every restore surface behind one digest-, lifetime-, and scope-verifying resolver. |
| PSH-003 | Repair AST signature fidelity; make structured passthrough explicit; add reversible structured forms only behind their own proof gates. |
| PSH-004 | Keep deterministic protected-span compression, but distinguish lexical estimates from model tokens and propagate budget failure. |
| PSH-005 | Enforce publish-before-marker and verify the actual retained object, including existing-target reuse. Cover stdin and all owner-managed lossy paths. |
| PSH-006 | Update evidence to reflect actual native packet materialization, rather than describing all budgeted preparation as CLI-only. Then fix and qualify materialized shared-budget accounting. |
| PSH-007 | A query-aware runtime consumer now exists, but opt-in is not proof. Bind policy to actual admission/freshness evidence; document which surfaces can request it. |
| PSH-008 | Qualify automatic tool/MCP egress per host and tool class. Native context reduction is not universal interception. |
| PSH-009 | Retain governed document reads. Demonstrate how a reduced source resolves to the exact relevant source version; do not equate document anchors with generic spill anchors. |
| PSH-010 | Make provider/transform outcomes typed and subordinate to planner policy. A refusal must not silently trigger a different lossy transform. |
| PSH-011 | Reclassify as PARTIAL. Select the largest *measured* complete eligible representation, not the largest representation with a small declared allocation. |
| PSH-012 | Keep the native H8 forwarding/schema repair. Complete nested schema, provenance, estimator compatibility, and supported-client parity tests. |
| PSH-013 | Define terminal exact/refused lanes and deterministic fallback ordering. If the exact fallback cannot fit, refuse/replan instead of truncating protected evidence. |
| PSH-014 | Test preservation of required values and relationships, not just approximate counts of identifiers/numbers. Include signatures, errors, negation, paths, and decision-driving code/data. |
| PSH-015 | Preserve planner order and explicit identity through transformations. A codec must not become a second relevance-ranking stage. |
| PSH-016 | Reconcile original, selected/materialized, model-delivered, recovery, and provider-reported usage with explicit units. |
| PSH-017 | Add opportunity denominators, execution/delivery/restore joins, failure classes, and externally graded quality evidence. |
| PSH-018 | Connect the command allowlist/adapter checks to the production CLI mode that claims them. |
| PSH-019 | Reclassify the strengthened delivery contract as PARTIAL. Carry the content-free selection receipt through native MCP and host delivery without injecting alternate representation bodies. |
| PSH-020 | Keep root confinement and strengthen shared store identity and artifact scoping. Test default CLI/service interoperability. |
| PSH-021 | Use shell-free argv and Git sanitization in the governed path; separate approved legacy shell semantics explicitly. |
| PSH-022 | Retain strict canonical `mr://anchor/...` parsing. Do not weaken it to compensate for storage misconfiguration. |
| PSH-023 | Reclassify as PARTIAL; enforce expiry and malformed/missing-metadata refusal consistently across CLI, HTTP, SDK and MCP. |
| PSH-024 | Implement bounded exact selectors. Define byte-span versus semantic-value retrieval precisely; do not directly port newline-normalizing selectors under an exact-byte claim. |

Evidence basis: [canon and implementation register][M01], [prior comparison][M03], [intake reconciliation][M04], and the call-site findings in sections 3–4.

### 5.3 Five new atoms, with bounded ownership

The new IDs are PSH-025–PSH-029. They are committed requirements **in this supplied revision**, not claims that their code is delivered. Their introduction authority is the user's request for the consolidated final plan and revised canon, recorded in the accompanying promotion receipt.

| New ID | Observable contract | Source-state assessment | Why it is not just another implementation detail |
|---|---|---|---|
| **PSH-025** | Qualify the current consumer's actual discovery/call access to the matching authorized artifact resolver before offloading. | MISSING | An original may exist in storage while the receiving agent has no callable recovery operation. This complements PUL-035's planner publication recheck; it does not replace it. |
| **PSH-026** | Preserve an explicit exact/exempt terminal disposition across preparation, rendering, recovery and re-entry. | PARTIAL | A valid first reduction can still be broken by a later lossy stage. Generic fallback ordering alone does not carry an exactness disposition through every consumer. |
| **PSH-027** | Admit optional reduction as a savings optimization only on measured positive final-wire savings; classify safety caps and unknown economics separately. | MISSING | Fitting a ceiling and saving resources are different acceptance conditions. PSH-011 measures fit; this subordinate gate prevents claiming an expanding or unmeasured transform as an optimization. |
| **PSH-028** | Expose and honor the artifact's expiry/retention promise, with explicit invalidation and no silent renewal. | PARTIAL | Read-time expiry refusal does not by itself establish how long a valid reference remains available or whether the consumer can see that lifetime. |
| **PSH-029** | Bound per-artifact publication/recovery resources under inherited limits/cancellation without publishing an incomplete original as exact. | PARTIAL | Bounding a displayed preview does not bound captured disk usage, whole-artifact restore allocation, parsing work or concurrent retrieval. |

The original PSH-001–PSH-024 IDs remain. A single existing group, PSH-G01, rolls up all 29. Every capability has one implementation entry and one explicit, still-pending qualification entry.

**Deliberately not new atoms:** the validator belongs to PSH-014; bounded segment decisions to PSH-017; representation/fidelity/recovery fields to PSH-016; shared language extraction to PSH-003; the native resolver implements PSH-002/024 and supports PSH-025; safe tool envelopes strengthen PSH-008; query proof strengthens PSH-007; selectors already belong to PSH-024. Command filters and codecs remain implementation choices. Stable packet prefixes and repeat suppression already have Pull owners (PUL-039 and PUL-037). [Pull ownership][M20-pull]

The old PSH-D005 decision is preserved as history. PSH-D006 explicitly supersedes only the parts now promoted into these five independent contracts; the rest of its anti-duplication/ownership rule remains.

### 5.4 Fidelity should have two dimensions

A useful contract is:

```text
inline fidelity:
  exact_bytes | equivalent_under_named_schema | projection

original recovery:
  not_needed | verified_retained_original | unavailable
```

This avoids claiming that a short lossy summary is itself lossless just because a disk file exists. It also avoids confusing JSON semantic equivalence with byte identity: key order, duplicate keys, number spelling, Unicode normalization, line endings, and whitespace may matter to different consumers.

For a self-contained codec, prove its declared inverse/equivalence. For a projection, retain the exact original under a verified handle, preserve the mandated inline evidence, and qualify task quality. For an exact lane, preserve original bytes and do not run additional lossy stages.

### 5.5 Cross-subsystem impact and integration boundary

| Owner | Required coordination | What this package does not do |
|---|---|---|
| Membrane protocol/host boundary | Shared receipt fields, native schema/discovery, real H8 observation, inherited deadline/cancellation, final host-delivery evidence. | Invent host capacity, add a second scheduler, or claim all hosts expose output rewriting. |
| Pull | Own source eligibility/membership, scope and freshness, representation policy, final publication recheck, stable prefixes (PUL-039), and repeat suppression (PUL-037). | Add a Push relevance planner or duplicate those Pull atoms. |
| Ledger | Reuse governed immutable source/snapshot facilities where qualified; maintain distinction between historical artifacts and current worktree sections. | Turn a document source reference into a generic command-output anchor without a versioned contract. |
| Blueprint | Reuse verified language/grammar metadata and source-span extraction where interfaces actually exist. | Claim the submitted “37 languages” are already supported by Push or introduce a second symbol resolver. |
| Adapt | Consume truthful bounded savings, restoration and task-quality observations through existing feedback authority. | Allow reducer self-reports to authorize automatic learning or policy changes. |
| Cortex | Continue owning durable knowledge; retain Push captures only under their artifact-lifetime policy. | Insert raw tool output, reduction telemetry or transient recovery artifacts into durable memory by default. |

Only the Push canon is supplied as a replacement. Other subsystem canons remain unchanged: their existing ownership is referenced, not renumbered or silently rewritten. Implementation changes will touch shared contracts and callers, but those do not automatically justify more capability atoms.

The canon checker requires matching register targets, receipt hashes, introduction provenance and generated indexes. The bundle supplies a new comparison receipt for the 26 non-retained rows and five additions to the existing New capability register. The three retained historical comparison/proof references and all old receipt files are unchanged. That avoids re-pinning historical receipts across the other six canons. [Canon checker][M21-checker]

## 6. Donor mechanisms and supplemental-source corrections

The original five-project comparison is retained below. Supplemental LLMLingua and context-compressor analysis is in section 6.8. Repository descriptions and benchmark numbers are not independently reproduced performance evidence. Selected source was inspected for Secondwind, Distil, Context Compress and the supplemental validator; RTK/Context Mode observations remain documentation-level observations from the earlier audit.

### 6.1 Comparison and adoption decision

| Repository | Relevant mechanism | Membrane action | Boundary / caution |
|---|---|---|---|
| **[orchetron/secondwind][D1]** | Proof-gated representations, net-cost gating, recoverable offload, a declared resolver, targeted selection. | **Highest-priority mechanism donor:** shared optimizer/recovery boundary, consumer-qualified offloading, typed refusal and exact lanes. | Do not import its relevance engine into Push. Exact restoration must use Membrane’s own scope and lifetime authority. |
| **[rtk-ai/rtk][D2]** | Command-aware compact output, automatic command routing on supported hosts, retained raw failure output. | Borrow output-shape knowledge and the clear distinction between automatic hooks and instruction-only adoption. | Command rewriting is not coverage of built-in file tools or arbitrary MCP tools. Do not inherit lossy defaults without raw retention and quality tests. |
| **[mksglu/context-mode][D3]** | MCP plus host-specific routing, explicit platform capability differences, execution/index/retrieval workflow, diagnostics. | Borrow capability granularity and installation/route verification ideas. | Do not duplicate Cortex/Pull/Ledger ownership. Current project is **Elastic License 2.0**, not a blanket permissive-code donor. |
| **[Open330/context-compress][D4]** | Command-aware filters, conservative/balanced/aggressive choices, explicit declined-filter state, nonempty fallback helper. | Adapt pure reducers only, behind typed policy, preserved originals, final-wire measurement, and exact fallback. | Its `wrap`/`filter` paths explicitly do not make removed output searchable by themselves. Copying them would not solve recoverability. |
| **[dshakes/distil][D5]** | Reversible structured views, cache-aware stability, task/decision-oriented quality qualification and explicit grader identity. | Borrow conservative shape admission, cache/quality evaluation design, and held-out task testing. | A recoverable compact table is not automatically an unambiguous typed lossless encoding. Do not transplant short handle formats or performance guarantees. |

### 6.2 Secondwind: the best match for the missing contract

At inspected commit `ab3888a4bbc43ec1ce080b2e29a3861a7cb5eaeb`, the optimizer separates compressed, offloaded, and kept-verbatim outcomes and has an explicit host-controlled offload mode. Its documentation says offloading is enabled only when the resolver is actually present. That is the right idea for Membrane’s currently incomplete agent recovery loop. [Optimizer source][D1-core] · [Project documentation][D1]

Its selector supports line ranges and field/index/key navigation, but the implementation uses `lines()` followed by newline joining and canonical JSON rendering. **That implementation is not an exact original-byte slice for CRLF/trailing-newline inputs or arbitrary JSON formatting.** Use it as a semantic-selection reference; adapt the contract and span handling for PSH-024 instead of claiming a direct port provides exact bytes. [Selector source][D1-select]

### 6.3 RTK: adoption is a separate capability

RTK documents automatic Bash-command rewriting, but explicitly says built-in Read/Grep/Glob tools bypass that hook. It also distinguishes hook/plugin support from instruction-based integrations, and its savings documentation distinguishes approximate output-token reduction from actual billing. These are useful disciplines for Push’s capability and telemetry UI. Its documented raw-output retention on command failure is also worth adapting without rerunning the command. [RTK documentation][D2]

**Take:** a small initial catalogue for Git status/diff/log, Cargo diagnostics/tests, search matches, and repetitive progress output. **Do not take:** a claim of universal adoption based solely on successful hook installation.

### 6.4 Context Mode: learn the coverage model, not the whole architecture

Context Mode documents different interception and lifecycle capabilities per host, including cases where event capture does not imply argument rewriting. Its diagnostics and installation verification are relevant to proving that a real tool path is active. Its index/search and session-memory functions overlap other Membrane subsystems, so they are not a reason to add another retrieval or memory owner inside Push. [Project and capability documentation][D3]

The current README declares Elastic License 2.0. Treat this as an architectural/behavioral reference unless the chosen source and intended use have been cleared; do not assume MIT just because an older fork or prior version used it. [License declaration][D3-license]

### 6.5 Context Compress: useful reducers, not a recovery solution

At inspected commit `27fedcf24f4f8506a7d179799161a90f5c883169`, `applyCommandFilter` routes based on command kind and uses explicit filter state. Its `withFloor` helper avoids converting a nonempty input to an empty response in the branches that use it. Those mechanisms map well to typed reducer eligibility and passthrough receipts. [Filter source][D4-filters]

The project explicitly warns that `wrap` and `filter` do not write the retrieval index. Therefore removed material is not magically recoverable. Keep Membrane’s raw-first contract above any borrowed reducer. Its repository declares MIT; because it identifies Context Mode ancestry, verify the actual files/revision and historical notices before copying rather than inferring either permission or infringement from the current upstream’s different license. [Documentation][D4]

### 6.6 Distil: conservatism and task-quality evidence

At inspected commit `067f0e577b607290119610041dcfeba55f32246f`, the structured folder checks schema uniformity and delimiter ambiguity, and distinguishes a view carrying a recovery handle from one used when no expansion tool is available. Its flat-fold path avoids null/missing ambiguity. These are good examples of refusing a codec outside its proven shape. [Structured source][D5-structured]

Do not label a compact view byte-lossless merely because it retains all displayed cells: the inspected scalar rendering and compact representation need a precisely defined type/format contract. Preserve Membrane’s full-strength digest identity rather than copying the sample’s short handle convention.

Distil’s documented cache and quality methodology is useful as an evaluation design, not a transferable guarantee: include a raw-versus-raw noise baseline, identify the grader, and test actual coding decisions/tasks rather than accepting a synthetic “certificate” as proof of deployed model behavior. [Project documentation][D5]

### 6.7 Absorption order and licensing gate

Recommended order: **verified recovery and exact lanes → final-wire sizing → command-aware safe reducers → reversible structured codecs → cache-stable reuse → qualified query-aware/semantic reduction.**

The consulted repositories declare Apache-2.0 for Secondwind, RTK, and Distil; MIT for Context Compress; and Elastic License 2.0 for current Context Mode. This is a source-screening observation, not blanket approval to copy every file or dependency. Preserve required notices and record the adopted file, commit, license, tests, and adaptations in provenance. [Secondwind][D1] · [RTK][D2] · [Distil][D5] · [Context Compress][D4] · [Context Mode][D3-license]

### 6.8 Supplemental donor claims: take the mechanism, correct the evidence

**context-compressor — validator pattern, not a proof implementation.** At source pin `f35898dac946e6a72cb112915cfcabb5c0c6f86c`, `validate.py` builds a normalized `universe` from the current segment objects, including the retained segment it later checks. It can therefore validate a changed retained segment against itself. It also lowercases and collapses whitespace. This is a source-level defect in the claimed validation guarantee, not evidence that the normal compressor necessarily fabricates text. Do not copy this check or describe it as byte-exact provenance proof. [Pinned validator][D6-validate]

The donor's `score.py` independently appends multiple tags, and `is_critical` includes constraint/decision/preference but not question. That does not match the submitted document's exclusive-first `constraint > decision > preference > question` claim. Membrane must define its own typed required-span policy; cue heuristics may conservatively preserve data but cannot create user authority or preference truth. [Pinned classifier][D6-score]

**LLMLingua — existing backend, not a new engine mandate.** Membrane already has a feature-gated local LLMLingua-2 ONNX path. Production routing, assets, protection behavior, actual estimator, deterministic fallback and held-out quality are the work to qualify. Some inspected native packet branches explicitly bypass ONNX. Do not introduce a parallel engine based on the assumption that only a generic heuristic exists. [Existing compressor][M22-compressor]

LLMLingua's documented `recover(original_prompt, compressed_prompt, response)` post-processes a model response with the original prompt available. It does not implement PSH-024's digest-bound, authorized, expiring artifact restore or exact byte selector. Original LLMLingua's perplexity-oriented approach and LLMLingua-2's token classifier also should not be flattened into one universal “two-step perplexity engine.” Use the existing backend as an optional local extractive mechanism under shared gates. [Microsoft documentation][D7-document] · [Project family][D7-readme]

The final plan therefore adopts the **independent preservation gate and decision trace**, not the submitted validator code; **existing learned-backend qualification**, not engine replacement; and **typed exact artifact selectors**, not a supposed LLMLingua recovery port.

### 6.9 Survey-only suggestions and exclusions

The attachment also names LangChain's compressor interface, Serena, GitNexus, Graphify, Octocode, SCIP and Potpie. Those names are retained as survey leads, not promoted as source-qualified donor implementations in this revision. No code adoption or completion claim is based on an unpinned survey name.

An admitted-query interface is useful, but a Rust type name alone cannot prove that an external caller's query, freshness or authority was verified. Keep constructors behind the existing admission boundary and validate deserialized requests. Symbol/range conventions may inform a selector, but authoritative symbol resolution stays with Blueprint/source owners. New source ranking from graph tools does not belong in Push.

**No generative summarization is added to Push.** Source-backed extractive views, qualified interface skeletons, exact/canonical codecs under named equivalence and recoverable projections remain distinct. An existing summary supplied by an upstream source does not authorize Push to generate another one. Model-based compression must stay local under this plan's correctness/privacy boundary and still pass deterministic preservation/recovery gates; model confidence is never an integrity certificate.

## 7. Proposed implementation architecture

### 7.1 One shared preparation owner—not another framework

Introduce a small owner above the existing pure transforms. Reuse an existing immutable blob/snapshot owner where it already provides the required guarantees; do not automatically create a second copy of Ledger’s storage.

Conceptually:

```text
prepare(request, capabilities, policy, recovery_store, estimator)
  1. Validate scope, authority, source identity, freshness and host capability.
  2. Classify exact/protected/structured/code/prose/protocol content.
  3. Obtain a verified immutable original before any recoverable projection.
  4. Materialize eligible representations using existing pure transforms.
  5. Validate mandated spans, identities and declared codec equivalence.
  6. Build the actual consumer delivery form, including necessary hints.
  7. Measure candidates with their real estimator basis.
  8. Select the largest complete eligible candidate that fits.
  9. Emit one delivery plus a content-free, versioned receipt.
```

Never let a transform choose new sources, enlarge a grant, claim freshness, execute an unapproved command, or silently change the planner’s order.

### 7.2 Minimum request/result contracts

The exact Rust names are illustrative, not claims that these types already exist.

| Object | Required information |
|---|---|
| `PushPrepareRequest` | Source reference/digest; content kind; repository/scope/session/task/turn/tool identity; planner selection/order; protected spans; requested policy; authority/freshness evidence; capacity reference; estimator; deadline/cancellation; consumer capabilities. |
| `PreparedRepresentation` | Representation ID; inline fidelity; original-recovery state; actual rendered payload; actual byte/token measurement; estimator identity; preserved-evidence result; transformation/version; typed refusal or fallback reason. |
| `RecoveryReference` | Opaque handle; immutable source digest; store/installation identity; owner scope/grant; media type/encoding; size; expiry/lease policy; available selectors. |
| `PushDeliveryReceipt` | Opportunity/execution/selection IDs; source and representation identities; input/materialized/delivered measurements and units; transform; fidelity; protected-evidence result; recovery reference/availability; terminal reason. |

Keep source content, secrets, command arguments, and arbitrary user text out of ordinary telemetry. A structured diagnostic request may expose appropriately authorized content separately.

### 7.3 Proposed recovery operation

`membrane_push_resolve` should be a read operation, not an execution operation. Suggested input:

```json
{
  "repository": "repository-id",
  "caller": {
    "root": "/authorized/worktree",
    "repositoryId": "repository-id",
    "scopeId": "session-scope"
  },
  "handle": "mr://anchor/<canonical-digest>",
  "selector": {
    "kind": "lines",
    "start": 120,
    "end": 160
  },
  "maxBytes": 16384
}
```

The operation should support a minimal initial selector set: whole bounded object, byte range, and line range. Add JSON field/index/key selectors after specifying their semantic and encoding rules. A typed selector is preferable to an unrestricted expression language.

Important rules:

- Verify the original before selecting. Return the original digest plus the selected span/value identity; a slice hash alone does not verify its parent.
- Preserve original line endings for exact text slices. A canonical semantic JSON projection must be labeled as such.
- Distinguish invalid selector, out-of-range, too-large, expired, corrupt, unavailable, and denied outcomes.
- A continuation must bind to the same immutable original and scope; it must not trigger re-execution or silently switch to the latest file.
- Mark returned exact material as terminal for Push composition. Do not create an expand → compress → expand loop.

### 7.4 Recovery lifecycle and storage policy

Define quotas and failure behavior before turning on broad automatic capture. Bound artifact size, per-session and total storage, concurrent work, and restoration output. Specify what happens when storage is full: keep exact inline content when it fits, or return a typed refusal; never emit an unbacked marker.

Use a lease/expiry policy that matches the advertised guarantee. A handle promised for a live session must not be evicted opportunistically without an explicit invalidation contract. Test restart, concurrent readers, duplicate writers, partial publication, deletion/purge, and platform-specific filesystem behavior.

Raw captures can contain secrets and sensitive repository data. Retention must respect the same access boundary as the original tool/source, and security redaction/denial must remain upstream of what an agent is permitted to recover. “Exact lane” is not a bypass around authorization.

### 7.5 Host integration strategy

For an owned Rust harness, call the shared preparation owner after existing tool execution and before model rendering. For supported external hosts, use a version-qualified hook/extension boundary. For generic MCP, expose explicit preparation/recovery where appropriate, but do not claim it intercepts unrelated tool servers.

Advertise separate capabilities:

```text
can_reduce_membrane_context
can_observe_remaining_capacity
can_reduce_own_tool_output
can_intercept_host_tool_output
can_intercept_external_mcp_output
can_resolve_push_artifacts
can_preserve_exact_results
can_observe_final_model_delivery
```

Each capability needs `available/unavailable/degraded`, a reason, host/version, evidence timestamp, and the route tested. Avoid one ambiguous `push_enabled=true` flag.

### 7.6 Cache stability without authority drift

For a repeated immutable block, retain the selected representation’s bytes for that representation identity rather than recompressing it differently each turn. Bind the identity to source digest, transform/version, fidelity policy, relevant query/policy identity, and estimator/renderer where required.

Recheck authorization and lifetime when reusing it. Byte stability is not permission to reuse revoked or stale material. If a new task requires a different view, treat that as a new explicit representation rather than silently mutating a historical one.

Optimize final-wire savings first. Account for extra tool calls and restores, and separately observe cache behavior and provider usage when available. A codec that saves bytes but adds prompt instructions or repeated recovery calls may not be a useful optimization.

### 7.7 Independent preservation gate — PSH-014, not a new atom

The validator must not use the transformed segments, classifier output, or generated report as its own ground truth. Its trusted inputs are a verified immutable original, the planner/source owner's mandatory evidence obligations, the selected transform's declared contract, and the proposed representation.

A suitable logical contract is:

```text
validate(original, obligations, representation, transform_contract)
  -> VerifiedRepresentation
  | ValidationFailure { typed_reason, bounded_failure_refs }
```

This is proposed interface vocabulary, not an assertion that those Rust types already exist.

The original identity includes its digest, byte length, encoding/media type, and relevant source version. Obligations name source spans or typed invariants, their authority/provenance, and whether they must be inline or may be restored. They are assessed **before** content is dropped. A reducer must not downgrade mandatory-inline evidence to “recoverable” to meet a budget.

For each source-backed output span, keep the original byte offset/range and compare the exact original slice with the output slice. Repeated identical text needs occurrence identity where location/order matters; substring membership alone is insufficient. Check coverage of every required span, the preserved order and relationships, and the representation's declared transformation semantics.

Generated elision markers, codec headers and recovery notices must be identified as generated metadata, not passed off as original evidence. For a structured codec, verify the named inverse/equivalence with conservative handling of duplicate keys, number spellings, null/missing values, escapes, line endings and type distinctions. Never label a canonical semantic JSON view as byte-identical to its original.

Do not infer complete semantic preservation from word counts or extraction alone. “Do not deploy” → “Do deploy” retains source words but loses the prohibition. A test name without its failure status, or a number attached to the wrong field, can also retain tokens while changing the evidence. The safety gate enforces explicitly required evidence; held-out task tests remain necessary for what that gate cannot prove.

**Minimal regression corpus:** mutate a retained span; remove a negation; change case in an identifier; swap two values; reorder a decision and rationale; retain a duplicate occurrence from the wrong source; insert a generated marker as alleged evidence; accept a self-referential validation universe. Each must fail the relevant declared invariant, not merely produce a warning while publishing the candidate.

Failure invalidates the candidate. Fall back through PSH-013's typed, bounded path; PSH-026 preserves any exact/refused disposition through subsequent stages. Re-measure any fallback under PSH-011. When no faithful complete result fits, return a typed refusal instead of inventing success.

### 7.8 Bounded per-segment decisions — PSH-017

For source spans considered by a transform, record a compact action such as `kept`, `projected`, `offloaded`, `exact_duplicate_reference`, `not_applicable`, or `refused`, together with source identity/range, typed reason, transform version and validation outcome. This is an audit of representation decisions, not a second scoring/ranking authority.

Keep detailed traces access-controlled and bounded. The routine model-facing receipt and telemetry carry IDs, offsets, counts, digests and reason codes—not original text, secrets, arbitrary paths or command arguments. When a detailed trace exceeds its budget, report the omitted-decision count and preserve aggregate validation coverage; do not pretend that every row was recorded. Trace overflow must not bypass the safety validator.

Join actual delivery and recovery to the selected representation ID. An evaluated-but-unused reduced candidate is not delivered savings. A restore can erase or reverse a local byte/token reduction, so report initial savings and observed restoration cost separately. Task success comes from an identified external verdict or remains unknown.

### 7.9 Shared fidelity, language metadata and expiry visibility

Store **representation kind** independently from **inline fidelity** and **original-recovery availability**. A skeleton is a representation kind; it may be a faithful interface projection but is not the full file. `Materialized` is a stage, not a fidelity grade. A selector capability describes what can be fetched, not whether the complete original remains available.

Use the existing shared protocol crate for the eventual wire types. That is an implementation-location choice, not a separate atom. Pull's coverage/sufficiency states remain distinct: `exact_bytes` does not mean “sufficient for the task,” and “partial coverage” does not necessarily mean corrupted bytes.

For language handling, inventory the actual Blueprint grammar metadata and interfaces, then reuse them only where compatible. Do not assume the example `blueprint::language_tables::get()` is a present Rust API. Sharing extension-to-language lookup is useful; each Push signature/AST extraction policy still requires its own fixtures. A recognized language with no qualified extractor must remain exact, not fall through to a prose compressor.

Expose `expiresAt`, observation time and a typed lease state with recovery references. A near-expiry notice can be emitted when the consumer receives or resolves the reference; it does not require a new asynchronous notification service. Renewal is explicit, authorized and bounded; neither access nor re-compression silently extends retention. Revocation or authorized purge may invalidate a lease explicitly and must never be represented as ordinary successful recovery.

### 7.10 Savings admission and resource bounds — PSH-027–PSH-029

For optional optimizations, compare the final raw and candidate delivery forms using the **same actual basis**:

```text
immediate_net_saving = measured_raw_delivery - measured_candidate_delivery
```

Include necessary wrappers, markers and decoding hints. Use the real host-delivery scope rather than assuming per-block token estimates are additive. With an unavailable/incompatible estimator, record unknown savings and do not claim an optimization passed this gate. Byte savings, model-token savings and monetary savings are separate measures.

This gate does not change the largest-complete-fit policy: when the authorized full representation fits and that policy selects it, retain full content. It also does not forbid a necessary safety cap merely because the cap adds metadata to a tiny result; classify that as safety behavior, not cost reduction. Do not import a donor's provider price table or empirical reopen prior as Membrane's measured result.

Define per-artifact and total retention bounds, request/slice bounds, concurrent recovery limits, parser/codec work bounds and inherited cancellation behavior. Enforce size limits before whole-file allocations; stream verification where appropriate without releasing unverified output. Verification may be cached only under a defensible immutable-object identity/integrity model—do not skip it merely because the pathname looked unchanged.

A storage or resource limit must produce a typed result: keep an authorized complete inline result when it fits, or refuse/mark unavailable. Never issue a recovery reference that silently points to a partially captured original. Cleanup and publication must tolerate cancellation, restart, concurrent writers and authorized invalidation. Reuse Membrane's existing scheduler/deadline owner rather than adding another one.

## 8. Delivery plan: repair first, then expand

### PR 1 — Freeze contracts and add failing regression fixtures

Pin the audited cases and establish the supported production transports. Add regression fixtures for planned-versus-materialized counts, recovery root mismatch, missing metadata, no-spill anchors, dropped H8, lost selection evidence, and damaged signatures. Add native MCP schema/transport parity fixtures rather than only direct helper tests.

**Exit:** the failure cases are reproducible in the project’s actual test environment; each has an owning canonical requirement and intended outcome. This audit itself has not executed those tests.

### PR 2 — Shared verified recovery and a real agent resolver

Implement the shared store/resolve boundary; connect CLI and HTTP; distinguish digest from handle; verify existing-object reuse; make retention/lifetime failures explicit. Add the narrow native MCP resolver and qualify it through normal discovery (PSH-025). Establish visible retention state (PSH-028) and per-artifact bounds (PSH-029) before broad capture. Implement byte/line selectors first; PSH-024 stays PARTIAL until its committed index/field/key semantics are also qualified.

**Exit:** one artifact produced through the normal path can be restored through every supported consumer with identical integrity/lifetime decisions. A resolver-less consumer cannot receive an offload-only result as recoverable.

### PR 3 — Honest materialized sizing and delivery receipts

Materialize before counting, preserve estimator identity, propagate budget failure, and select on actual final rendering. Add the subordinate final-wire savings gate (PSH-027). Remove redundant model-facing copies. Carry a content-free selection receipt through native/compatibility projections without exposing alternate bodies. Keep representation kind, inline fidelity and recovery availability distinct.

**Exit:** every admitted representation has a defensible measured bound, and a trace joins selection to actual delivery. Capacity refusal remains explicit when protected content cannot fit.

### PR 4 — Safe composition, query proof, and skeleton fidelity

Replace string-based fallback decisions with typed outcomes and preserve terminal exact/exempt disposition (PSH-026). Implement the independent source-span/obligation validator under PSH-014, with bounded segment decisions under PSH-017. Derive query-aware eligibility from admitted/freshness evidence. Repair AST extraction, investigate shared language metadata, and wire governed argv adapters without silently changing approved shell semantics.

**Exit:** refusals cannot be undone by another lossy stage; exact/restore material remains exact; signatures pass the new fixtures; command execution happens once under the correct owner.

### PR 5 — One qualified automatic output integration

Choose the owned harness or one supported installed external host. Add the real output boundary, ensure it sees only eligible text payloads, preserve tool-call/result/error structure, and publish capability-specific diagnostics. Retire obsolete launch paths or bridge them to the native contract.

**Exit:** a real tool call produces a reduced model-facing result, a matching receipt, and a callable exact recovery; unsupported tool classes remain honestly marked unsupported. Installed hook evidence is collected separately from unit tests.

### PR 6 — Measured optimization donors and quality qualification

Add a small command-aware reducer set, then reversible structured codecs for homogeneous/nested records and repetitive logs where named equivalence can be proven. Complete PSH-024 selector coverage. Coordinate stable representation reuse with Pull PUL-039 and proper telemetry joins. Qualify the existing optional LLMLingua-2 backend and held-out task behavior before enabling more aggressive modes; do not create a parallel compressor.

**Exit:** improvements show net delivered benefit without violating recovery, protected evidence, or chosen task-quality gates. No donor benchmark percentage is treated as Membrane’s own result.

**Do not block the repair sequence on:** a new model, universal proxy support, another memory database, a large codec catalogue, automatic Adapt policy changes, or a redesign of all six Membrane subsystems.

### 8.1 Canon/package integration order

The bundle contains the final plan at `docs/architecture/subsystems/push.md`, the complete revised ledger at `docs/canon/push.md`, two new provenance receipts and the five preservation-register additions. A guarded local installer checks the baseline commit and original canon blob and refuses to overwrite conflicting work. It is dry-run by default; it makes no network requests, stages no files and commits nothing.

After reviewing/applying the package in the intended checkout, stage the new receipts and changed files so the existing checker can verify their Git blob hashes and introduction references. Then run:

```bash
node scripts/ci/check-atomic-canons.mjs --write
node scripts/ci/check-atomic-canons.mjs
```

The first command regenerates the authoritative pending/canon indexes; the second verifies them. The bundle does not hand-edit those generated indexes or remove checker invariants to make new atoms pass. The full checker needs the actual repository and its preserved historical Git objects, not a synthetic subset.

A successful document/schema check does not qualify implementation. Execute the focused and installed-path tests below after implementing each PR and update the owning implementation, verification, qualification and delivery fields independently. Do not set a new atom DELIVERED merely because its schema/tool name exists.

## 9. Qualification and regression matrix

The following tests are proposed acceptance criteria. They were **not run** during this source audit.

| Area | Essential fixtures | Passing condition |
|---|---|---|
| Native reachability | Real `tools/list` and `tools/call`, stdio and HTTP, active Hub. | Correct required schema and successful native path with a valid host observation. |
| H8 | Missing, malformed, stale, session mismatch, task mismatch, incomplete coverage, incompatible estimator. | Typed refusal or accepted valid observation; no invented capacity. |
| JS compatibility | Input file/client/hook to actual resident route. | Typed envelopes preserved, or path explicitly removed from supported installation. |
| Budget | Structured passthrough, protected overflow, unsupported source, verbose wrappers, dense JSON, multilingual text. | The measured final candidate fits; declared metadata alone is never sufficient. |
| Recovery identity | CLI default → HTTP/MCP, explicit store override, nested current directory, restart. | The same live handle resolves against the intended store without hidden path assumptions. |
| Publication | Existing corrupt target, concurrent writes, partial metadata, interrupted publish. | No successful handle is issued for an unverified/incomplete retained object. |
| Integrity/lifetime | Modified bytes, bad/missing metadata, expired handle, unsupported schema. | Consistent typed failures across CLI, HTTP, SDK, and MCP. |
| Exactness | Binary bytes, CRLF, trailing newline, Unicode, whitespace-sensitive content. | Exact-byte operations round-trip; semantic projections are clearly distinguished. |
| Selectors | Valid/missing field, empty value, array index, duplicate matching keys, out-of-range lines, oversize slices. | Bounded deterministic semantics; absence is not mistaken for an empty value or another record. |
| Scope | Cross-repository/session/grant, revoked access, traversal/symlink cases under the stated threat model. | No unauthorized bytes are returned. |
| Composition | Query refusal on code, exact restore, already-compressed marker, parser fallback. | No second lossy pass after a terminal exact/refused outcome. |
| Skeletons | Multiline Python/decorators; TS destructuring/exports/TSX; Rust data/method shapes; incomplete edits. | Required spans and declared interface facts survive; unsupported input is handled honestly. |
| Execution | Spaces, quotes, metacharacters, Unicode arguments; Git env; nonzero exit; stderr; cancellation. | Correct argv/approval boundary and one execution only. |
| Egress protocol | Paired tool IDs, errors, mixed text/non-text, streaming/long-running operations. | Protocol semantics preserved; unsupported modes do not hang or silently rewrite. |
| Telemetry | No sink, disabled policy, no opportunity, refused transform, restore failure, repeated delivery. | Distinct states, explicit units, no raw-content leakage, no double-counted savings. |
| Quality | Raw-vs-raw baseline; raw vs each representation on held-out tasks. | Predeclared task/evidence gates met with the grader and uncertainty identified. |

### 9.1 Supplemental acceptance cases

| Owning atom | Required additional case | Acceptance |
|---|---|---|
| PSH-014 | Validator checks modified output against itself. | Independent original/source spans reject the mutation. |
| PSH-014 | Negation/status/value survives as tokens but not as the required relationship. | Typed obligations fail; candidate is not published. |
| PSH-017 | Huge segment trace or unavailable telemetry sink. | Trace truncation/sink state is explicit; validation still runs; no invented zero/outcome. |
| PSH-025 | Tool exists in code but is not negotiated/discoverable by this consumer. | No offload-only result is emitted as recoverable. |
| PSH-026 | Exact restore re-enters the output reducer. | Exact/exempt disposition survives without a second lossy pass. |
| PSH-027 | Marker/decoder hint makes the compact payload larger. | Optional optimization is declined or classified non-beneficial, with the actual basis. |
| PSH-028 | Artifact is reused, purged, renewed or near expiry. | Visible lifetime remains truthful; renewal is explicit; authorized invalidation is typed. |
| PSH-029 | Capture fills storage or cancellation occurs mid-publication. | No partial original is advertised as exact; cleanup and terminal reason are observable. |
| PSH-003 / PUL-039 | A recognized language lacks a qualified extractor; a representation is resent. | Exact fallback for unsupported extraction; approved stable prefix/representation does not drift. |

### 9.2 Task-level corpus

Use actual workload shapes rather than generic prose only: Rust build failures, Cargo test output, Windows and macOS paths, Git diffs and renames, search hits, JSON API results, stack traces, source interfaces, policy text, contradictory evidence, and numbers/negations that change the required action.

Grade whether the agent finds the relevant error, proposes the correct change, preserves specified values, notices contradictions, asks for missing evidence appropriately, and completes the task. Track restores, redundant tool calls, and extra latency. A large compression ratio with incorrect decisions is a regression.

For learned/query-aware modes, separate development and held-out examples, record model/provider/settings and the real runner, and include an unchanged-input baseline to estimate inherent variation. Choose risk thresholds before examining the evaluation result; do not import someone else’s synthetic certificate as a universal guarantee.

### 9.3 Useful existing test entry points

The repository already defines MCP, canon, host-contract, and wider test commands. In a prepared checkout, the following are useful starting points; they are **commands to run during implementation, not results of this audit**:

```bash
pnpm run test:mcp
pnpm run check:atomic-canons
node scripts/generate-host-contracts.mjs --check

cargo test --manifest-path engine/Cargo.toml -p membrane-runtime --locked push::
cargo test --manifest-path engine/Cargo.toml -p membrane-mcp --locked
```

Use the repository’s documented build environment and full release/qualification workflow after focused fixes; its `test:all` script uses the project’s `rightkit cargo` wrapper. Add integration tests for the newly connected paths rather than assuming these focused commands establish installed-host qualification. [Package scripts][M19]

## 10. What “done” should mean

Push is ready to be described as properly integrated for a named host/tool class only when the normal installed path demonstrates all of the following:

1. The host observes the capability and supplies real authority/capacity inputs.
2. Eligible output crosses the intended boundary automatically or through the explicitly documented tool surface.
3. Reduction preserves its declared evidence/fidelity contract and fits its measured delivery budget.
4. Any advertised original is already retained, scoped, verified, and resolvable by that consumer.
5. Exact recovery does not re-execute a command or trigger another lossy compression loop.
6. Content-free receipts connect opportunity, transformation, selection, delivery, recovery, and qualified outcome evidence.
7. Unsupported or unavailable paths are reported honestly rather than counted as active coverage.

That is a stronger and more useful completion criterion than “the compressor exists,” “the unit test passed,” or “the plugin installed.”

## 11. Audit limitations and evidence handling

The original audit used pinned-source/caller review and selected donor source/documentation; it did not run Membrane or reproduce donor benchmarks. This consolidation rechecked the repository head (unchanged), the canon/checker schemas, cross-subsystem ownership and the supplemental validator source. A direct container source download failed because GitHub DNS was unavailable; connected GitHub reads supplied the repository evidence.

No Membrane Cargo/Node test suite, live Hub, installed host configuration, final provider request, full canon checker or release qualification was executed during this consolidation. The bundle's own structure, IDs, generated receipt hashes and integration-helper behavior are validated separately; those document checks are not implementation evidence. See `VALIDATION.md` in the bundle for the exact artifact checks performed.

The report distinguishes direct source observations, source-derived counterexamples, proposed implementation interfaces, survey-only donor suggestions and unexecuted acceptance criteria. It does not claim an exhaustive repository audit. Native H8 schema/forwarding is explicitly credited; retained JavaScript mismatches are not generalized to the native route.

Historical evidence is preserved only for PSH-004, PSH-012 and PSH-022. The four superseded delivered claims are stale under the revised acceptance boundary. Other amended/new rows carry PENDING evidence and qualification. `CURRENT_INCOMPLETE` in the new source-comparison receipt is not a donor benchmark result. No release, billing, task-quality or successful implementation receipt is fabricated.

The final plan is a synthesis, not a verbatim adoption of the submitted deep-dive. Its useful proposals and the rejected/corrected claims are recorded explicitly. The attachment's terminology is retained where sound; differences in interpretation and design are stated rather than silently substituted.

---

## Source map

All `M*` links below point to the audited Membrane commit. Donor source samples are pinned where inspected. Documentation observations are inherited from the earlier 5 September 2026 review unless marked as a supplemental recheck; they describe the projects, not the user’s installed hosts. Microsoft documentation was rechecked during consolidation. Supplemental context-compressor source is pinned below.

### Membrane evidence groups

| Group | Primary evidence |
|---|---|
| Scope, commitments, prior intake | [Current Push canon][M01]; [older competitive receipt][M03]; [archive/master-atom reconciliation][M04]. Historical receipts are not current release qualification. |
| Public/manual transformation and recovery | [CLI dispatch][M05]; [CLI Runc/Restore][M05-restore]; [spill publication][M09-publish]; [argv adapter and shell executor][M09-run]. |
| Runtime preparation and measurement | [Representation-plan construction][M06]; [block transform dispatch][M06-transform]; [budget compressor and estimator][M07-budget]; [marker verifier][M07-verify]; [skeleton renderer][M08-render]; [skeleton budget fallback][M08-budget]. |
| Resident HTTP and store configuration | [Service boundary][M10-header]; [anchor-root configuration][M10-roots]; [HTTP expansion][M10-expand]; [direct federation wrapper][M10-federate]; [HTTP compression][M10-compress]; [native Hub/MCP startup][M10-native]. |
| Native agent surface | [Executor, context projection, and document-section read][M11]; [advertised and negotiated tools][M12]; [resource registry][M13]. |
| JavaScript compatibility/host paths | [Client input translation][M14-input]; [client result projection][M14-output]; [resident context hook][M15]; [JavaScript toolset registry][M16-tools]. |
| Native federation and policy | [Request-time H8 admission][M17-h8]; [selection and published result][M17-selection]; [query-aware policy construction][M17-policy]. |
| Measurement and validation tooling | [Push telemetry][M18]; [repository scripts][M19]. |

### Donor evidence and inspection depth

| Project | Evidence used | Inspection boundary |
|---|---|---|
| Secondwind | [Live project documentation][D1]; [optimizer core][D1-core]; [selector implementation][D1-select]. Code pin: `ab3888a4bbc43ec1ce080b2e29a3861a7cb5eaeb`. | Selected code and documentation reviewed; no full audit, local build, or benchmark replication. |
| RTK | [Live project documentation][D2]. `develop` was the observed default branch; its ref resolved to `84f629d7195ced9e5ce4422f5b2901422ae601a9`. | Documentation-level comparison. The attempted `src/tee.rs` fetch did not resolve, so this report does not claim to have audited its tee implementation. |
| Context Mode | [Live project documentation][D3]; [current license declaration][D3-license]. | Documentation-level host-capability and execution/indexing comparison, not a pinned implementation audit. |
| Context Compress | [Live project documentation][D4]; [command-filter sample][D4-filters]. Code pin: `27fedcf24f4f8506a7d179799161a90f5c883169`. | Selected command-dispatch/fallback code reviewed; no full qualification. |
| Distil | [Live project documentation][D5]; [structured-compaction sample][D5-structured]. Code pin: `067f0e577b607290119610041dcfeba55f32246f`. | Selected codec code reviewed; this pin differs from the older Distil pin in Membrane’s archive intake. |
| context-compressor | [Validator][D6-validate] and [classifier][D6-score], pin `f35898dac946e6a72cb112915cfcabb5c0c6f86c`. | Selected source review; validator rechecked in consolidation. No donor benchmark or runtime qualification. |
| LLMLingua | [Microsoft API documentation][D7-document], [project family][D7-readme], and [existing Membrane backend][M22-compressor]. | Recovery semantics and existing integration clarified; no learned-model benchmark was run. |

[M01]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/canon/push.md
[M03]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/provenance/foundation/2026-08-31-competitive-comparison/push.md
[M04]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/provenance/foundation/2026-08-31-master-atom-intake/push-review.md
[M05]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/cli.rs#L3420-L3825
[M05-restore]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/cli.rs#L3670-L3825
[M06]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/selection.rs#L1-L400
[M06-transform]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/selection.rs#L395-L560
[M07-budget]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/compress.rs#L300-L430
[M07-verify]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/compress.rs#L230-L260
[M08-render]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/skel.rs#L1-L155
[M08-budget]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/skel.rs#L155-L280
[M09-publish]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/runc.rs#L390-L575
[M09-run]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/runc.rs#L770-L925
[M10-header]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/serve.rs#L1-L100
[M10-roots]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/serve.rs#L215-L370
[M10-expand]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/serve.rs#L410-L465
[M10-federate]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/serve.rs#L2870-L3025
[M10-compress]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/serve.rs#L4750-L4940
[M10-native]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/serve.rs#L5500-L5750
[M11]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/mcp_executor.rs#L480-L700
[M12]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-mcp/src/tools.rs#L1-L195
[M13]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-mcp/src/resources.rs#L1-L140
[M14-input]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/mcp/client.mjs#L1-L250
[M14-output]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/mcp/client.mjs#L250-L420
[M15]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/mcp/host/context-adapter.cjs#L1-L245
[M16-tools]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/mcp/toolsets.mjs
[M17-h8]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/pull/federation.rs#L143-L345
[M17-selection]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/pull/federation.rs#L345-L395
[M17-policy]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/pull/federation.rs#L495-L544
[M18]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/telemetry.rs
[M19]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/package.json
[D1]: https://github.com/orchetron/secondwind
[D1-core]: https://github.com/orchetron/secondwind/blob/ab3888a4bbc43ec1ce080b2e29a3861a7cb5eaeb/crates/optimize/src/lib.rs#L1-L195
[D1-select]: https://github.com/orchetron/secondwind/blob/ab3888a4bbc43ec1ce080b2e29a3861a7cb5eaeb/crates/optimize/src/select.rs
[D2]: https://github.com/rtk-ai/rtk
[D3]: https://github.com/mksglu/context-mode
[D3-license]: https://github.com/mksglu/context-mode#license
[D4]: https://github.com/Open330/context-compress
[D4-filters]: https://github.com/Open330/context-compress/blob/27fedcf24f4f8506a7d179799161a90f5c883169/src/filters.ts#L1-L155
[D5]: https://github.com/dshakes/distil
[D5-structured]: https://github.com/dshakes/distil/blob/067f0e577b607290119610041dcfeba55f32246f/distil/compress/structured.py#L1-L175

[M20-pull]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/canon/pull.md
[M21-checker]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/scripts/ci/check-atomic-canons.mjs
[M22-compressor]: https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/push/compress.rs
[D6-validate]: https://github.com/ingridtoulotte/context-compressor/blob/f35898dac946e6a72cb112915cfcabb5c0c6f86c/ctxcomp/validate.py
[D6-score]: https://github.com/ingridtoulotte/context-compressor/blob/f35898dac946e6a72cb112915cfcabb5c0c6f86c/ctxcomp/score.py
[D7-document]: https://github.com/microsoft/LLMLingua/blob/main/DOCUMENT.md
[D7-readme]: https://github.com/microsoft/LLMLingua
