# Memory Sentinel

Memory Sentinel is a bounded, content-free read model for Hub diagnostics. It shows lifecycle counts (active, demoted, superseded, expired), proposal and contradiction counts, scratchpad and working expiry, task criteria, evidence validity, and gate state.

The runtime projection is schema `1` and caps identifier arrays at 64. Missing or stale inputs render `unknown`; process presence is not readiness evidence. Hub v1 is read-only: this view exposes no save, clear, promote, resolve, or other data-plane mutation.
