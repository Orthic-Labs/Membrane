# Membrane memory lifecycle

Doc Spine candidate provider (`doc_candidate_provider.rs`) exposes only the shadow-selection seam (`DocCandidateProvider::select_shadow`, `RegisteredDocCandidateProvider`) and the `is_doc_provider_enabled()` flag check. Unreachable planner-admission helpers were removed; see `docs/archive/superseded/reference/deferred-surfaces.md` (S-10) for historical context & `docs/pending/README.md` for current work.

See `engine/crates/membrane-runtime/src/doc_candidate_provider.rs` for the remaining shadow-selection seam and `doc_shadow.rs` for the frozen replay disposition.
