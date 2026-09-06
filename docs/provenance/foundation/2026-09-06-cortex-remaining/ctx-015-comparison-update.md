# CTX-015 competitive disposition update

Baseline comparison revision: `5be1f9fd443c22e69128988601963cee090823f9`.
Freshness: `2026-09-06`.

This receipt updates only CTX-015, whose implementation was
recorded CURRENT_INCOMPLETE against comparison revision
`30b3c211ae874f369bed3fe92eb94b2fc5acbb16`. The prior disposition rested on a
named implementation gap; that gap is now closed, so the disposition can no
longer honestly assert incompleteness. Neither claims release qualification, and
competitive closure stays UNRESOLVED until the RELEASED boundary is reachable.

For CTX-015 the gap was real and is now fixed rather than reinterpreted. A
cited-verdict feedback row previously ranked on any non-empty `verdict_ref`
string, so "receipt-bound" was a claim the code did not keep. Resolution against
the durable verdict event is now enforced at persistence, fail-closed.

| Atom | Scope | Competitive disposition | Best mechanism | Current evidence | Donor evidence | Gap / action |
|---|---|---|---|---|---|---|
| CTX-015 | COMMITTED | UNRESOLVED | Recall feedback that adjusts mutable usefulness signals only, where ranking eligibility requires a verdict reference resolving to a durable verdict event bound to the same trace and candidate, and a verdict cannot be replayed across candidates. | `DELIVERED / FOCUSED_PASS`; `resolve_cited_verdict` in `engine/crates/membrane-runtime/src/store.rs` gating `record_feedback_observed`; consumer `engine/crates/membrane-runtime/src/mcp_executor.rs:1431-1456`; managed CI run `34063008859`. | Prior comparison named no donor mechanism enforcing receipt resolution before feedback may rank. | Keep UNRESOLVED until RELEASED-bound qualification closes CTX-Q015. |
