# Hub actions

Post-v1 Hub actions are request builders, not dispatchers. Available actions: restart, reconcile, token rotation, proposal review, quarantine restore, & update application.

Each request requires a receipt-bound exact `hub.action.*` grant, 16–128 character explicit confirmation nonce, & closed action-specific identifiers. Update requests bind exact release-generation SHA-256. UI shows `awaiting-trusted-runtime`; it never presents success, mutates local state, or fabricates a receipt.

Trusted runtime alone executes request & returns immutable, content-free receipt bound to action ID, grant receipt, request digest, outcome, time, runtime identity, & repair path. Runtime owns authorization, nonce replay prevention, dispatch, rollback, & repair.
