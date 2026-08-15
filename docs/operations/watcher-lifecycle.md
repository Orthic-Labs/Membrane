# Watcher lifecycle

Cortex is sole owner of `cortex-watch.mjs`, its process lifecycle, &
`~/.cortex/watchman.pid`. Membrane never spawns, adopts, supervises, or writes
watcher state.

## Retirement

MBR-201's former Membrane-supervisor watcher policy is retired. Shipped
supervisor configuration is schema v2 & deliberately contains no
`watcherPolicy`. Cortex manages its own watcher lifecycle; Membrane consumes
Cortex only through published APIs.

## See also

- `docs/plans/orthic/SEAM-CONTRACT.md` — D-S09 watcher ownership.
