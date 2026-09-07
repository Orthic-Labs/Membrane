# CTX-019 competitive disposition update

Baseline comparison revision: `fb6c695c884e4648c5f09de1a086adb5a3e7ea5a`.
Freshness: `2026-09-06`.

This receipt updates only CTX-019 after checkpoint promotion was wired through every production intake (MCP `checkpoint_promote` and CLI `Promote`) into the durable governed proposal queue with proposal-only semantics preserved. It does not claim release qualification. The earlier donor comparison proved no donor mechanism that outperforms this proposal-first promotion contract; competitive closure remains unresolved until release-bound qualification is available.

| Atom | Scope | Competitive disposition | Best mechanism | Current evidence | Donor evidence | Gap / action |
|---|---|---|---|---|---|---|
| CTX-019 | COMMITTED | UNRESOLVED | Checkpoint content promotes only as a normal governed knowledge proposal through the pending queue and signed review; never direct admission. | `DELIVERED / FOCUSED_PASS`; `engine/crates/membrane-runtime/src/cortex_lifecycle.rs` `promote_checkpoint` plus MCP and CLI consumers; focused promotion test green in managed CI. | Prior comparison: no donor proves a stricter proposal-first checkpoint contract. | Keep competitive disposition UNRESOLVED until RELEASED-bound qualification closes CTX-Q019. |
