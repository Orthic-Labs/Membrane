# Membrane memory lifecycle

Doc Spine candidate provider (`doc_candidate_provider.rs`) exposes only the shadow-selection seam (`DocCandidateProvider::select_shadow`, `RegisteredDocCandidateProvider`) and the `is_doc_provider_enabled()` flag check. Unreachable planner-admission helpers were removed; see `docs/reference/deferred-surfaces.md` (S-10) for what existed, why it was unreachable, and what a future contract must decide before wiring a real request path.

See `engine/crates/membrane-runtime/src/doc_candidate_provider.rs` for the remaining shadow-selection seam and `doc_shadow.rs` for the frozen replay disposition.
