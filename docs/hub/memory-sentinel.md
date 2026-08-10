# Memory Sentinel

Memory Sentinel is Crypt's memory-lifecycle read model: which memories are
currently authoritative (`active`) versus which have moved out of that state
(`demoted`, `superseded`, `expired`), plus the pending queues around that
lifecycle (`proposals`, `contradictions`) and expiry-tracked scratch state
(`scratchpad`, `working`, `taskCriteria`). It is a bounded, **content-free**
read model for Hub diagnostics — it never carries memory text, only counts
and identifiers. That is a deliberate privacy boundary: the Hub surface can
show what state the memory store is in without ever rendering what an agent
actually remembered.

## Source and shape

The projector is `memory_sentinel_view::project` in
`engine/crates/membrane-runtime/src/memory_sentinel_view.rs`
(`SCHEMA_VERSION = 1`, `MAX_ITEMS = 64` — identifier arrays are truncated at
64 and each list also reports its true `count` independent of how many ids
were kept). It reads an already-assembled report and never mutates
data-plane state. Fields:

- `lifecycle` — `SentinelCounts { active, demoted, superseded, expired }`,
  each an `Option<u64>`. `None` means unknown, not zero.
- `proposals`, `contradictions`, `scratchpad`, `working`, `taskCriteria` —
  each a `SentinelList { count, ids }`.
- `evidence` — `EvidenceState { state, valid, reason }`, defaulting to
  `unknown` / `false` / `"unknown — no evidence"` when the report is
  missing that shape.
- `gate` — `GateState { state, reason, authoritative }`, defaulting the
  same way, plus `authoritative: false`.

`apps/membrane-hub/src/memory-sentinel.mjs` is a hand-maintained JS mirror
of the same projection (`memorySentinelViewModel` / `renderMemorySentinel`)
used by the Hub UI. It duplicates the Rust struct's field names, defaults,
and the 64-item cap by hand — there is no shared codegen between the two,
so a change to one (a new field, a changed default, a different cap) will
not propagate to the other automatically. Keep them in sync manually when
either changes.

## Current honest status: no producer

The projector and its JS mirror are real and exercised by unit tests, but
**nothing in this repository currently calls `project` with a real report**.
`engine/crates/membrane-runtime/src/hub_inputs.rs` wires the Hub's
`sentinel` input to `not_instrumented()` explicitly, with this note at the
call site:

> `memory_sentinel_view::project` ... is real but has no producer.
> `startup_sentinel_masked` in main.rs is an unrelated boolean, not backed
> by this view. Truthful state is `not_instrumented`; wiring a producer is
> separate future work.

So today, the Hub's sentinel resource reports `not_instrumented`, not the
lifecycle/evidence/gate data above — the schema exists and is tested, the
data path to populate it does not yet exist. Wiring a real producer is
in-progress, separate work (tracked as Lane B in this session); this
document describes the shape the producer will need to fill, not a
currently-live feature.

## Not the same thing: `startup_sentinel_masked`

`apps/membrane-hub/src-tauri/src/main.rs` has its own, unrelated
`StartupGate` (`active: AtomicBool`) that can mask Hub startup: while
`gate.active()` is true and the polled/cached snapshot looks like
"source not connected" (`source_not_connected_snapshot`), `poll_snapshot`
and `initial_poll` return `Err("startup_sentinel_masked".into())` instead of
surfacing that snapshot, until the gate is later closed with
`gate.finish()`. This *is* real teeth — it can withhold Hub startup truth
during the connection grace window. But it is a simple boolean startup gate
over Hub's own snapshot polling, not backed by `memory_sentinel_view` and
not the same as the `gate: GateState` field above (which belongs to the
lifecycle projection and, per the "no producer" status, is not currently
populated either). The shared word "sentinel" names two unconnected
mechanisms; do not assume a code change to one affects the other.

## Contract

The runtime projection is schema `1` and caps identifier arrays at 64.
Missing or stale inputs render `unknown`; process presence is not readiness
evidence. Hub v1 is read-only: this view exposes no save, clear, promote,
resolve, or other data-plane mutation.
