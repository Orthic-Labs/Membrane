# G5 Lane B — MemRight provider integration note

Lane owner: minimax/rightcontext-memory
Branch: `minimax/rightcontext-memory`
Base: local `main` HEAD `7e7b557` (per task spec)

## What this lane adds

A new module that exposes `produce_candidate_set(store, task, scope,
max_candidates)` reading eligible `MemoryEntry` rows from `MemoryStore` and
returning v1 `ContextCandidateSet` records (Layer 7,
`trustClass=agent_verified`, `instructionPolicy=data_only`). The provider is
in-process with the planner and never authenticates to itself or reads a
token file.

## Files

- `tools/memright/crates/memright/src/memory_provider.rs` — new module
- `tools/memright/crates/memright/tests/memory_provider.rs` — new contract tests
- `tools/memright/crates/memright/src/store.rs` — additive 8-line change
  (one new `pub fn db(&self) -> &MemDb` accessor at line ~1648)

## Required integration-owner wiring

This lane did not edit the forbidden hot files (schema, `main.rs`,
`lib.rs`, `serve.rs`, `memdb.rs`, `setup-workspace.py`, `.gitignore`).
The integration owner applies ONE wiring commit after merge:

```diff
--- a/tools/memright/crates/memright/src/lib.rs
+++ b/tools/memright/crates/memright/src/lib.rs
@@ -6,6 +6,7 @@
 pub mod compress;
 pub mod memdb;
+pub mod memory_provider;
 pub mod prep;
 pub mod runc;
 pub mod scope;
```

Optional follow-up (one-line ergonomic change): re-export `produce_candidate_set`
and the `ContextCandidateSet` types so the planner service can `use memright::memory_provider::*;`.

```diff
 pub use store::{MemoryEventContext, MemoryStore};
+pub use memory_provider::produce_candidate_set;
```

After this single wiring commit, the integration tests' `#[path =
"../src/memory_provider.rs"]` shim in `tests/memory_provider.rs` becomes
redundant; the test file can drop the `#[path]` and import via
`memright::memory_provider::produce_candidate_set` directly.

## Tests

`cargo test --manifest-path tools/memright/Cargo.toml --test memory_provider`
runs 17 contract tests:

- candidate shape matches v1 contract (layer/sourceKind/trustClass/policy)
- scope filter only admits chain members (self + ancestors + global)
- supersession collapses duplicates onto highest score
- no cross-root leaks via sibling scopes
- demoted entries excluded and recorded as omissions
- empty store yields empty candidates without panic
- max_candidates caps admitted count but preserves omissions
- candidate provenance carries access_count and last_seen
- instructionPolicy is data_only and resolver is provider name
- trace_id is stable and omits task text
- does_not_authenticate_to_itself_or_read_a_token (structural pin)

Plus 6 unit tests inside the module for `is_demoted`, `dedup_key`,
`estimate_tokens`, `trimmed_text`, `trace_id_for`, and an empty-store
produce_candidate_set smoke.

## Contract discipline

- All `ContextCandidate` + `ContextCandidateSet` field names use camelCase to
  match `tools/lib/context-contracts.schema.json` byte-for-byte.
- `providerScore` is clamped to `[0, 1]`.
- `sourceHash` is 64-char hex (no `sha256:` prefix; the schema pattern
  accepts both).
- `traceId` is SHA-256 of `(task || 0x00 || scope)` — never the raw task text.
- Stable reason codes: `demoted`, `superseded`, `out_of_scope`.
- The provider does NOT search the repository or read files; it reads ONLY
  through `MemoryStore::entries(...)` and `MemoryStore::scopes()`.
- The provider does NOT authenticate to itself; the public surface is one
  free function with no token/bearer parameter.

## Verifiability

- `cargo test --manifest-path tools/memright/Cargo.toml` — full workspace,
  243 tests pass (94 store + 8 db_first + 4 embedder_probe + 17
  memory_provider + 117 memright-core + 3 memright-format + others).
- The forbidden hot-file list is untouched: `git diff --stat
  main...HEAD -- tools/lib/context-contracts.schema.json
  tools/memright/crates/memright/src/{main,lib,serve,memdb}.rs
  tools/setup-workspace.py .gitignore` returns empty.