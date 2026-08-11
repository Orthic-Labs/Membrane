# Membrane memory lifecycle

Doc Spine candidate provider (`doc_candidate_provider.rs`) exposes only the shadow-selection seam (`DocCandidateProvider::select_shadow`, `RegisteredDocCandidateProvider`) and the `is_doc_provider_enabled()` flag check. The planner-admission wrapper (`plan_with_doc_shadow`) and the opt-in admission function (`maybe_admit_doc_candidates`) were removed as unreachable dead code — see `docs/reference/deferred-surfaces.md` (S-10) for what exists, why it was unreachable, and what a future contract needs to decide to wire it for real.

See `engine/crates/membrane-runtime/src/doc_candidate_provider.rs` for the remaining shadow-selection seam and `doc_shadow.rs` for the frozen replay disposition.
