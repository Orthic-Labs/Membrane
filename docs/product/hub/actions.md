# Hub actions

Post-v1 Hub actions are request builders, not dispatchers. Available actions: restart, reconcile, token rotation, proposal review, quarantine restore, & update application. Each has its own distinct `hub.action.*` capability & its own `repair/hub/<action>` repair path — none are shared across actions.

Each request requires a receipt-bound exact `hub.action.*` grant, 16–128 character explicit confirmation nonce, & closed action-specific identifiers. Update requests bind exact release-generation SHA-256. UI shows `awaiting-trusted-runtime`; it never presents success, mutates local state, or fabricates a receipt. `buildHubActionRequest` fails closed (`unavailable`) on an unknown action, a missing/malformed confirmation nonce, a missing capability grant, or an invalid payload — never an optimistic success.

Every built request carries a `requestDigest`: a sha256 over the canonical (key-sorted) actionId, capability, capability-receipt ID, confirmation nonce, & payload.

Trusted runtime alone executes the request & returns an immutable, content-free receipt bound to action ID, grant receipt, request digest, outcome, time, runtime identity, & repair path. Runtime owns authorization, nonce replay prevention, dispatch, rollback, & repair.

`applyRuntimeReceipt(request, receipt)` is the only path from a pending request to a rendered outcome, & it never trusts a claimed outcome on its own:

- The receipt must be shape-valid against `schemas/registry/hub/actions/hub-actions.v1.json#runtimeReceipt` (closed outcome enum, ISO timestamp, `repair/hub/<action>` path, bounded identifiers).
- The receipt must be cryptographically bound to the exact request: matching `actionId`, `capabilityReceiptId`, `requestDigest`, & `repairPath`. A forged, replayed, or wrong-action receipt is rejected as `unavailable` / `receipt-not-bound-to-request`, never accepted.
- A bound receipt whose outcome is `rejected` renders as `rejected`, never `proven` — the Hub never upgrades a runtime rejection into a success.
- Only a bound, shape-valid receipt with outcome `applied`, `repaired`, or `rolled-back` renders as `proven`.
- Every outcome — `unavailable`, `rejected`, or `proven` — surfaces the action's `repairPath`, so a failed or rejected action always has a visible repair route.

`renderHubActionStatus` is a read-only DOM projection: it only ever sets `root.innerHTML`, & it HTML-escapes every rendered evidence field.
