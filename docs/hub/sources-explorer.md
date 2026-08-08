# Sources explorer

Sources explorer is a read-only Hub projection (`sources-explorer.v1`). Runtime supplies repository identity, release/index generation, observed clocks, provider readiness, parser capability, recent contribution, bounded paths, & bounded 2D neighborhoods.

Readiness is authoritative only when provider evidence says so. Hub never infers liveness from process existence, timestamps, or a populated path list; missing evidence renders `unknown`.

Runtime bounds paths at 64 entries & neighborhoods at 32 entries (64 neighbors each). Every row carries explicit evidence or `unknown`, so cached or partial snapshots remain inspectable without being presented as healthy.
