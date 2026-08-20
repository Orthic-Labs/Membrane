# Memory Sentinel

Memory Sentinel is Cortex's memory-lifecycle read model: which memories are
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

The legacy Hub renderer was a hand-maintained JS mirror
of the same projection (`memorySentinelViewModel` / `renderMemorySentinel`)
used by the Hub UI. It duplicates the Rust struct's field names, defaults,
and the 64-item cap by hand — there is no shared codegen between the two,
so a change to one (a new field, a changed default, a different cap) will
not propagate to the other automatically. Keep them in sync manually when
either changes.

## Current status: producer wired in source, not yet in the running Hub

A producer now exists. `engine/crates/membrane-runtime/src/memory_sentinel_producer.rs`
(commit `a639ec44`) builds the report from the live Cortex database —
`memories.lifecycle_state`, `memory_quarantine`, `transform_opportunity_log`,
and `context_feedback` — and `hub_inputs.rs` now wires the Hub's `sentinel`
input to it:

```rust
match build_sentinel_report() {
    Some(report) => /* Available */,
    None         => Unavailable { reason: "missing_input" },
}
```

It fails closed: a missing database, missing table, or unreadable row yields
`Unavailable{reason:"missing_input"}`, never a healthy-looking empty result.
Note `missing_input` is a distinct reason from `not_instrumented` — the first
means "the source could not be read", the second means "this was never built".

Two honest caveats:

1. **Not live yet.** The deployed Hub binary predates `a639ec44`, so the
   running service does not serve this data. Source-fixed is not runtime-fixed;
   it takes a rebuild and install.
2. **Partial coverage.** `scratchpad`, `working`, and `taskCriteria` are always
   absent — an instrumentation gap, *not* zero activity. Reporting them as empty
   would read as "nothing pending", which would be false. `demoted` maps to the
   quarantine count and `proposals` to pending transform recommendations, not to
   literal same-named columns.

Prior revisions of this document said Sentinel had no producer. That was true
when written and is now false; it is corrected here per the rule that fresh code
evidence outranks stale documents.

## Not the same thing: `startup_sentinel_masked`

The legacy desktop startup path had its own, unrelated
`StartupGate` (`active: AtomicBool`) that can mask Hub startup: while
`gate.active()` is true and the polled/cached snapshot looks like
"source not connected" (`source_not_connected_snapshot`), `poll_snapshot`
and `initial_poll` return `Err("startup_sentinel_masked".into())` instead of
surfacing that snapshot, until the gate is later closed with
`gate.finish()`. This *is* real teeth — it can withhold Hub startup truth
during the connection grace window. But it is a simple boolean startup gate
over Hub's own snapshot polling, not backed by `memory_sentinel_view` and
not the same as the `gate: GateState` field above (which belongs to the
lifecycle projection and is still not populated by the producer described
above). The shared word "sentinel" names two unconnected
mechanisms; do not assume a code change to one affects the other.

## Contract

The runtime projection is schema `1` and caps identifier arrays at 64.
Missing or stale inputs render `unknown`; process presence is not readiness
evidence. Hub v1 is read-only: this view exposes no save, clear, promote,
resolve, or other data-plane mutation.
