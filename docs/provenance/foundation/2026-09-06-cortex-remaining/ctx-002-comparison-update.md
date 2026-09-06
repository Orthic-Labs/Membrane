# CTX-002 competitive disposition update

Baseline comparison revision: `fb6c695c884e4648c5f09de1a086adb5a3e7ea5a`.
Freshness: `2026-09-06`.

This receipt updates only CTX-002 after the ordered pre-gate was frozen end-to-end in repository-owned code. It does not claim focused verification (managed CI proof pending) or release qualification. The earlier donor comparison proved no donor mechanism that outperforms this ordered governance gate; competitive closure remains unresolved until release-bound qualification is available.

| Atom | Scope | Competitive disposition | Best mechanism | Current evidence | Donor evidence | Gap / action |
|---|---|---|---|---|---|---|
| CTX-002 | COMMITTED | UNRESOLVED | Ordered schema/scope/producer/DLP/epistemic/stable-ID pre-gate with receipt-visible evidence before semantic admission. | `DELIVERED / PENDING`; `engine/crates/membrane-runtime/src/cortex_lifecycle.rs` ordered pre-gate plus `membrane_knowledge_propose`/`membrane_memory`/`membrane_temporal_fact` contract updates; focused tests `ctx002_ordered_pregate_rejects_each_dimension_before_admission` and executor propose paths updated. | Prior comparison: Hindsight validates retain inputs through a live API but lacks the same independent ordered governance gates; no donor proves this six-dimension fail-closed contract. | Keep competitive disposition UNRESOLVED until RELEASED-bound qualification closes CTX-Q002. |
