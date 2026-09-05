# Push final canon promotion and source reconciliation

Date: **5 September 2026**. Baseline repository revision: `75c257ad711d19ffce69258d132a45dbffa9b4ac`. Baseline Push canon blob: `fa618d7c6dfc2d043fa5d9793179360ec0f8b3a3`.

## Authority and scope

The user requested a final consolidated Markdown plan and, where the findings affect the canon or introduce atoms, the revised canon file. This package records that specification-authoring authority. It does not assert that the user asked to commit, push, deploy, alter the installed application or run the new code. No such remote or installed changes were made.

The supplied canon commits 29 requirements in the specification: the 24 existing IDs and five additional IDs below. Implementation, verification, qualification and delivery remain separate; `COMMITTED` scope is not `DELIVERED` implementation. All 29 release qualification rows remain PENDING.

## Input lineage

The final architecture document consolidates these conversation artifacts with the follow-up source review. Input digests identify the supplied copies; they are not code or test receipts.

| Input | SHA-256 |
|---|---|
| `Membrane_Push_Audit_and_Improvement_Plan.md` | `bdca2d7d942759d51877404bc8e1e45c48179d8dedb2b1eed3b782fe602a10d3` |
| `membrane-push.md` | `008ab1896977ed7654f5a6fad641f70183da69184d990b82541099b1ecfa7572` |

Additional source checks: repository main remained at the baseline; current Push canon and checker schema were read; Pull PUL-035/PUL-037/PUL-039 ownership was inspected; the supplemental context-compressor validator was re-read at `f35898dac946e6a72cb112915cfcabb5c0c6f86c`; Microsoft LLMLingua recovery documentation was rechecked. No new application test execution is claimed.

## New capability introductions

| Introduced ID | Origin | Observable behavior | Authority/evidence |
|---|---|---|---|
| PSH-025 | User-requested Push final reconciliation | Before offloading content, prove that the current consumer can discover and invoke the authorized recovery operation against the matching artifact store; otherwise return a complete inline result or typed refusal. | Explicit request for final implementation plan and revised canon, 2026-09-05; `docs/provenance/foundation/2026-09-05-push-final-reconciliation/promotion.md` |
| PSH-026 | User-requested Push final reconciliation | Carry an explicit exact/exempt disposition through all Push stages so exact reads, restored results & refused reductions cannot enter a second lossy transform; authorization remains enforced. | Explicit request for final implementation plan and revised canon, 2026-09-05; `docs/provenance/foundation/2026-09-05-push-final-reconciliation/promotion.md` |
| PSH-027 | User-requested Push final reconciliation | Admit an optional reduction as a savings optimization only when its fully rendered representation has measured positive net savings under the declared basis; classify safety caps and unknown economics separately. | Explicit request for final implementation plan and revised canon, 2026-09-05; `docs/provenance/foundation/2026-09-05-push-final-reconciliation/promotion.md` |
| PSH-028 | User-requested Push final reconciliation | Expose a recovery artifact’s declared expiry/lease state and honor its retention promise until expiry or an explicit authorized invalidation; renewal must never happen silently. | Explicit request for final implementation plan and revised canon, 2026-09-05; `docs/provenance/foundation/2026-09-05-push-final-reconciliation/promotion.md` |
| PSH-029 | User-requested Push final reconciliation | Bound Push artifact publication and recovery resource use by explicit byte/work/storage limits and inherited cancellation, returning typed limit outcomes without publishing incomplete recovery as exact. | Explicit request for final implementation plan and revised canon, 2026-09-05; `docs/provenance/foundation/2026-09-05-push-final-reconciliation/promotion.md` |

## Independent closure and ownership

PSH-025 qualifies consumer discovery/invocation and store binding for Push offloads; it complements rather than duplicates PUL-035's planner publication recheck. PSH-026 propagates exact/exempt disposition through re-entry, rather than relying only on generic fallback behavior. PSH-027 checks optional optimization benefit using the measured representation, not source eligibility or task utility. PSH-028 owns the visible artifact retention promise, separately from PSH-023's read-time expiry refusal. PSH-029 limits artifact-specific resource use under Membrane's existing scheduler/deadline ownership.

No new atoms are created for a validator module, per-segment report, language table, fidelity enum, native resolver name, compression model, structured codec or command filter. Those are mechanisms or acceptance refinements of existing contracts. Repeat suppression and reusable prefix semantics remain under Pull PUL-037/PUL-039. Existing PSH-D005 is retained as historical authority; D006 supersedes it only for these five named promotions.

## Source-state reclassification

| Atom | Before | This revision | Evidence boundary |
|---|---|---|---|
| PSH-001 | DELIVERED / FOCUSED_PASS | PARTIAL / STALE | No-spill handles and unverified published-object reuse prevent the strengthened publication claim. |
| PSH-011 | DELIVERED / FOCUSED_PASS | PARTIAL / STALE | Planned token totals do not establish actual materialized/final-delivery fit. |
| PSH-019 | DELIVERED / FOCUSED_PASS | PARTIAL / STALE | Receipt exists internally but the strengthened end-to-end transport contract is not met. |
| PSH-023 | DELIVERED / FOCUSED_PASS | PARTIAL / STALE | Missing/malformed expiry metadata and the CLI path lack uniform refusal. |

PSH-004, PSH-012 and PSH-022 retain the original historical focused proof and comparison references, without new test/competitive/release claims. All other capability Evidence cells are PENDING. The new comparison receipt records the source-review disposition of the other 26 rows; its content hash is computed from the actual supplied bytes. Old receipts and other subsystem canons are not edited.

## Submitted deep-dive reconciliation

Accepted with refinement: independent required-evidence validation; bounded segment action/reason reporting; shared qualified language metadata; visible expiry; typed admitted-query boundary and separately represented fidelity/recovery states.

Not adopted as written: the self-referential normalized donor validator; exclusive critical-class precedence claims; LLMLingua recover as exact artifact restoration; the claim that the backend is absent; `H8 × turns` as a session budget; mixed stage/kind/fidelity enums; inline `?sel=` changes to strict opaque anchors; universal egress/expiry claims; immediate combined JSONPath/CSV/symbol selector scope.

No generative summarizer, remote execution endpoint, separate memory system, relevance planner or budget authority is introduced by this revision.

## Verification status

The package's own Markdown/register/ID/hash integrity checks are recorded in `VALIDATION.md`. The actual repository checker, historical preservation validation, runtime tests and installed-host release qualification must be run in the real checkout. A package check is not a successful code test. No application result or receipt is fabricated.
