# PULL-B lane report — impl qualification

Lane scope: budget modes, faithful delivery, exact recovery, protected
evidence, publication fencing, delivery receipts/acknowledgement for
`engine/crates/membrane-runtime/src/pull/**` and the historical
`src/push/**` implementation Pull exposes via `#[path]` modules.
Coordinator owns git state, canon registers, and Rust builds. No
cargo/rightkit, installs, activations, host config, git mutations, or
canon-register edits were performed. Syntax of edited files was checked
with `rustfmt --emit stdout` (parse-only) — type-level verification is a
coordinator build dependency.

## ATOM TABLE

Statuses: IMPLEMENTED+VERIFIABLE = mechanism confirmed by inspection and
covered by an in-tree executable test row; IMPLEMENTED-UNVERIFIED =
mechanism present but the atom's full behavior or released-boundary claim
is not verified by this lane; GAP-REMAINS = atom's required end state is
not met.

| Atom | Status | Evidence |
|---|---|---|
| PUL-027 eligible representation selection under mode | IMPLEMENTED-UNVERIFIED | `push/packet_selection.rs`: same measured full/reduced_1/floor ladder now serves both modes — `select_packet_for_h8_with_recovery` (host-fit) and new `select_packet_for_token_budget` (bounded-response); plan validation preserves protected items + resolver/evidence lineage before selection; largest fitting complete representation is chosen, never a truncated one. Native path: `pull/federation.rs` `select_packet_for_h8_with_recovery` call (~:1130) and ambient path (~:499-540). Test row: `push_residual_qualification.rs`, packet_selection unit tests. |
| PUL-028 response budget vs host capacity reporting | IMPLEMENTED-UNVERIFIED | New `insert_budget_mode_fields` (`pull/federation.rs`) emits `budgetMode`, `requestedBudgetMode`, `responseBudget{value,unit,estimatorBasis,provenance}` and `hostCapacity{coverage:complete|unavailable,...}` on every Pull envelope arm; `deliveredResponse{renderedTokens,packetTokens,fitsBudget}` on ambient path; `packetReduction.selection_receipt` carries selected/remaining tokens. Response budget and observed/unknown host capacity are separate fields — capacity is never invented. |
| PUL-029 grant/policy re-observation before emission | IMPLEMENTED+VERIFIABLE | `post_fusion_publication_fence_until` re-reads the scope grant from catalog immediately before emission (`federation.rs:~1216` `fence_packet_emission`); grant admission itself is re-checked at request setup (`admitted_publication_grant_until`, :1417). Test row: `pull_post_merge_acceptance.rs`, `pull_residual_qualification.rs`. |
| PUL-030 typed `policy_changed`, no stale-authorized packet | IMPLEMENTED+VERIFIABLE | `fence_packet_emission` → `NativeRouteError::PolicyChanged` → `policy_changed` response (mcp_executor preserves the code at :1771). No packet bytes are emitted on that arm. |
| PUL-031 receipt completeness | IMPLEMENTED-UNVERIFIED | Envelope carries `fusionReceipt`, `correctiveRetrieval`, `publicationFence`, `insufficientConfidence`, `suppressionReceipts`, `packetReduction`, `budgetReduction` (omission reasons + dropped count), `requirementEvidenceMap`, BM10 accounting merge, `federationMetrics`, `gatewayStageTimingsMs`, and now mode/budget/host-capacity/delivery fields. Full atom vocabulary (candidate journey, named/versioned mechanisms, coverage) is emitted; per-atom released-boundary qualification remains the coordinator's build/test pass. |
| PUL-032 bounded content-free observations | IMPLEMENTED+VERIFIABLE | `push/telemetry.rs` records counts + unit strings only; stage timings are ms durations; refusal kinds are stable enums; block text never enters receipts. |
| PUL-033 versioned `insufficient_confidence` | IMPLEMENTED+VERIFIABLE | `insufficientConfidence` arm in native route and `unchanged_context`/insufficient handling in `mcp_executor.rs:1777-1795`; searched-lane counts in omissions/federation metrics. |
| PUL-035 resolver availability re-observed pre-publication | IMPLEMENTED-UNVERIFIED | Resolver capability is proven at plan build (`packet_selection.rs:61` `can_resolve` gate) and unresolved resolver surfaces produce `consumer_resolver_unavailable` detail (`federation.rs:~2020`); final grant fence runs after selection. A dedicated resolver re-probe distinct from selection is not separately evidenced. |
| PUL-036 authorization-input reconciliation before emission | IMPLEMENTED+VERIFIABLE | `publication_grant_lookup` re-read at `post_fusion_publication_fence_until` reconciles admitted vs current grant observation; changed reconciliation → `policy_changed`, packet withheld. |
| PUL-037 scoped suppression + eligibility restoration | IMPLEMENTED+VERIFIABLE | `pull/delivery_state.rs` + `pull/delivery_acknowledgement.rs`: H10 acknowledgement mandatory, H9 loaded-context identity/epoch/digest binding, process-local cache alone cannot authorize, cache loss/restart/compaction restores eligibility. `suppressionReceipts` emitted; emptied packets early-return with typed receipts. Test row: `pull_acknowledgement.rs`. |
| PUL-041 workspace aggregate budget | IMPLEMENTED-UNVERIFIED | `mcp_executor.rs:1388-1443` fans out per-repository `workspace_budget_shares` with per-target `requestId`/identity retained. Bounded-response is unreachable there — the surface hard-requires H8 (:1377-1387). See PATCH REQUESTS. |
| PUL-042 resolver-only evidence only for negotiated hosts | IMPLEMENTED+VERIFIABLE | `negotiated_consumer_resolvers` (`federation.rs:1646`) intersects caller-declared resolvers with runtime-owned surfaces only; `consumer_resolvers` gates resolver-backed eligibility in `envelope_from_ccs`; tools schema declares `consumerCapabilities.resolvers` enum. Test: `consumer_resolver_negotiation_intersects_runtime_owned_surfaces` (:2910). |
| PUL-043 exact byte restoration through scoped resolver | IMPLEMENTED+VERIFIABLE | `push/recovery.rs` `resolve_with_control` — scope-authorized, digest-verified, confined; `push/api.rs` binds task/session identity and rejects malformed selectors. Test rows: `push_residual_qualification.rs`, `serve.rs` recovery tests. |
| PUL-046 content-addressed externalization | IMPLEMENTED+VERIFIABLE | `RecoveryStore::publish` content-addressed artifact + verify + recovery marker committed on the reduced representation (`packet_selection.rs:84-99`); spill path in `push/runc.rs` fsync/rename + metadata (size, expiry, marker). |
| PUL-049 governed large-read preparation contract | IMPLEMENTED+VERIFIABLE | `push/delivery.rs` `prepare_with_control` + `push/runc.rs` apply the same reversible contract to governed large source reads: exact source-version + scope binding, immutable retained original, typed limits. Test rows: `push_end_to_end.rs`, `push_qualification.rs`. |
| PUL-050 two explicit budget modes | IMPLEMENTED-UNVERIFIED | Route mode resolution added (`federation.rs` `declared_budget_mode` + mapping): `budgetMode` canonical, `budgetPolicy` legacy spelling; supplied H8 → host-fit, absent H8 → bounded-response under declared caller/configured budget, invalid supplied H8 → typed refusal under both modes (never discarded). Host-fit requires trusted/fresh/exact/identity-matched H8 via `parse_request_time_h8` and never downgrades. Bounded path runs `select_packet_for_token_budget` / H8-tightened selection and refuses over-budget typed. `push/api.rs` no longer `.ok()`-discards invalid supplied H8 (→ `push_h8_invalid`). `egress::fit_native_response_to_budget` measures the serialized MCP tool result under a declared budget. MCP tool surface parity is patch-requested (BLOCKERS). |
| PUL-051 same mode across every fallback | IMPLEMENTED-UNVERIFIED | Ambient hook resident+one-shot, explicit one-shot, and the native/HTTP route all funnel through `run_federate_value`/`native_route_response_*` which now apply the resolved mode uniformly; typed `request_time_selection_refused` (`h8_unavailable`/`h8_invalid`/`h8_identity_mismatch`/`packet_reduction_refused`/`budget_insufficient`) propagates through hook detail and HTTP refusals instead of silent fallback. `hook_diagnostics.rs` no longer drops malformed supplied H8 into configured_cap. `NoRepresentationFits` below minimum-viable is the protected-evidence insufficiency refusal. |
| PUL-052 mandatory evidence preservation validation | IMPLEMENTED+VERIFIABLE | `push/fidelity.rs` `protected_lines`/`validate` against immutable original bytes; plan refuses empty protected set violations, duplicate identities, floor above full (`NoViableFloor`); floor representation keeps protected lines (`packet_selection.rs:87-98`). |
| PUL-053 evidence order + atomic grouping | IMPLEMENTED+VERIFIABLE | Representations preserve `packet.blocks` order; reductions map block-by-block without reordering; planner ordering policy (`place`) runs before reduction. No reordering path exists in the ladder. |
| PUL-054 strict canonical `mr://anchor/` | IMPLEMENTED+VERIFIABLE | `push/recovery.rs` anchor parse → `RecoveryError::InvalidAnchor` on non-canonical syntax (test `serve.rs:~8280` asserts `InvalidAnchor`/`NotFound`). |
| PUL-055 expired recovery refused on all transports | IMPLEMENTED+VERIFIABLE | `recovery.rs` TTL checked at resolve; missing/malformed lifetime metadata is a typed error, never unlimited lifetime; same store serves MCP/HTTP/CLI resolve. |
| PUL-056 bounded exact selectors | IMPLEMENTED+VERIFIABLE | `push/api.rs` `Selector` parse (`Whole` default; invalid → `push_invalid_selector`); `resolve_with_control` enforces `maxBytes` bound. |
| PUL-057 exact/exempt disposition through stages | IMPLEMENTED+VERIFIABLE | Block `protected`/`recoverable`/`resolver`/`delivery_stage`/`delivery_class` fields carried through every representation; exact/no-op reductions stay exact (`packet_selection.rs:70-83` — a second lossy transform never runs after refusal or failed span proof). |
| PUL-058 positive net savings admission | IMPLEMENTED+VERIFIABLE | `packet_selection.rs:105-107`: `reduced_tokens >= full_tokens` collapses to `full`; `floor >= reduced` collapses to `reduced`; savings-negative candidates skipped at :82. |
| PUL-059 recovery lease state | IMPLEMENTED+VERIFIABLE | Artifact expiry in recovery marker (`expiresAt`), `resolver_probe` exposes lease state, retention honored until expiry or authorized invalidation (`recovery.rs`). |
| PUL-060 bounded artifact resource use + cancellation | IMPLEMENTED+VERIFIABLE | `MAX_ARTIFACT_BYTES` aggregate/byte limits (`packet_selection.rs:41`, `recovery.rs`); deadline + cancellation threaded through publish/resolve/prepare (`*_with_control`); typed `RecoveryError::{Limit,Cancelled,Unavailable}` outcomes. |

## CHANGES

Owned-path edits (5 files):

- `engine/crates/membrane-runtime/src/pull/federation.rs`
  - Added `PullBudgetMode { BoundedResponse, HostFit }`, `BudgetFit`,
    `declared_budget_mode` (canonical `budgetMode` + legacy `budgetPolicy`
    spellings; unknown/conflicting declarations → typed
    `invalid_budget_mode` refusal), `declared_response_budget`
    (`responseBudget`/`responseBudgetTokens`/`maxTokens`; non-positive →
    `invalid_response_budget`), `insert_budget_mode_fields`
    (`budgetMode`/`requestedBudgetMode`/`responseBudget`/`hostCapacity`
    on every envelope arm), and `rendered_block_text`.
  - Route (`native_route_response_*`): mode resolution replaces the
    `configured_cap` special-case and unconditional H8 parse. Supplied H8
    is strictly validated under both modes (never discarded); host-fit
    requires it; bounded-response proceeds without it and still applies a
    supplied+validated H8 as a tightening ceiling. Mode receipts added to
    all three host-fit payload arms.
  - `run_federate_value` takes `BudgetFit`; the emitted packet is always
    fitted — H8 ladder selection when a validated ceiling exists (plus
    declared-budget bound on top), `select_packet_for_token_budget`
    otherwise; `deliveredResponse` measured under `o200k_base/1`;
    over-budget → `request_time_selection_refused:budget_insufficient`.
  - `hook_mode_federate_with_observation` now validates and
    identity-binds (sessionId + taskId) a supplied ceiling before use;
    failure → typed `h8_invalid`/`h8_identity_mismatch` refusal.
- `engine/crates/membrane-runtime/src/push/packet_selection.rs`
  - Added `select_packet_for_token_budget`: the same measured
    full/reduced/floor ladder, protected invariants and
    `NoRepresentationFits` typed refusal under a declared
    `o200k_base/1` token budget instead of an H8. Receipt names the
    declared budget (`budget://declared/<n>`), not invented host identity.
- `engine/crates/membrane-runtime/src/push/egress.rs`
  - Added `fit_native_response_to_budget(envelope, budget_tokens)`:
    identical serialized-MCP-tool-result measurement (wrappers +
    `deliveryMeasurement` receipts included, iterated to fixpoint) under
    a declared budget; `hostCapacity.coverage:"unavailable"` reported,
    never invented; over-budget/over-cap → `RecoveryError::Limit`, never
    truncation. Unit test `budget_gate_measures_the_same_wire_shape_…`.
- `engine/crates/membrane-runtime/src/push/api.rs`
  - Supplied `remainingContextCeiling` is strictly parsed before the
    operation; `parse_request_time_h8` failure → `push_h8_invalid` on
    every operation (including identity-free `probe`). The previous
    `.ok()` discard and the identity-gated `push_h8_invalid` branch are
    gone.
- `engine/crates/membrane-runtime/src/hook_diagnostics.rs`
  - A supplied-but-unparseable/invalid host observation now returns a
    typed refusal outcome (`host_observation_invalid` detail) instead of
    being silently filtered into `configured_cap`.

## PATCH REQUESTS

Required on non-owned surfaces so the committed modes reach every
projection; the Pull planner/route changes above are already in place.

1. `engine/crates/membrane-runtime/src/mcp_executor.rs`
   - `membrane_context` (:1774) and the workspace fanout (:1377-1387)
     hard-require `remainingContextCeiling` (`context_capacity_invalid`).
     Required change: forward `budgetMode`/`responseBudget` (advertised in
     tools.rs), treat absent H8 as bounded-response (no pre-gate), keep
     strict H8 semantics for host-fit, and call
     `pull::egress::fit_native_response_to_budget` instead of
     `fit_native_response` when the effective mode is bounded-response
     (:1580, :1616, :1793, :1830). Refusal propagation at :1759-1772
     already preserves `kind`/`reason` — no change needed there.
   - Workspace fanout should pass the resolved mode into each child body
     so per-target requests stay in the request's declared mode.
2. `engine/crates/membrane-mcp/src/tools.rs`
   - `membrane_context` schema (:114-126) marks `remainingContextCeiling`
     required and documents universal H8. Required change: advertise
     `budgetMode` (`bounded_response`|`host_fit`), `responseBudget`/
     `responseBudgetTokens`, keep H8 optional for bounded-response and
     required-by-contract for host-fit; document that supplied-but-invalid
     H8 refuses rather than being dropped.

## BLOCKERS

- **Rust build/test execution**: prohibited in this lane
  (`docs/rules/rightkit.md` — coordinator/RightKit owns cargo). All five
  edited files pass `rustfmt` parse; type-level verification and the
  in-tree test rows named above require a coordinator
  `rightkit cargo test -p membrane-runtime` pass.
- **MCP/stdio end-to-end parity**: blocked on the `mcp_executor.rs` +
  `tools.rs` patch requests above; until applied, the context tools still
  demand H8 for every request, so bounded-response is reachable only via
  the HTTP `/federate` route, the resident hook path, and the one-shot
  hook path.
- **Complete-rendered-response measurement scope**: the ambient/hook
  bounded gate measures the emitted packet representation (serialized
  packet tokens + rendered block text) because hook receipts are
  diagnostics, not host-injected content; the strict
  wrappers-and-receipts measure is implemented in
  `fit_native_response_to_budget` for the serialized tool result and is
  pending the mcp_executor wiring above.
- **PUL-035 dedicated resolver re-probe**: resolver capability is proven
  at plan build and re-fenced by the grant lookup; a distinct final
  availability probe immediately before emission is not separately
  evidenced and may need a small follow-up if the qualification requires
  a second probe rather than the plan-time proof.
- **PUL-019-style vocabulary dependency** (PULL-A lane): none of this
  lane's changes alter refusal/coverage vocabularies; no interaction.
