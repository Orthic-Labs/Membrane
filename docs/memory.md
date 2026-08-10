# Membrane memory lifecycle

Doc Spine candidate provider (`doc_candidate_provider.rs`) is **disabled by default** (`MEMBRANE_DOC_PROVIDER_ENABLED` unset → shadow-only, byte-identical to pre-CU-19 state). Enable explicitly:

```
MEMBRANE_DOC_PROVIDER_ENABLED=1 cargo test -p membrane-runtime doc_candidate_provider
```

When disabled, `plan_with_doc_shadow` emits `admitted_to_planner=false` and `maybe_admit_doc_candidates` returns `None`. When enabled (`MEMBRANE_DOC_PROVIDER_ENABLED=1`), candidates that pass freshness and task-class gates appear in the reviewed-learning queue via `maybe_admit_doc_candidates` (opt-in fixture: flag set → candidate appears).

See `engine/crates/membrane-runtime/src/doc_candidate_provider.rs` for the live provider seam and `doc_shadow.rs` for the frozen replay disposition.
