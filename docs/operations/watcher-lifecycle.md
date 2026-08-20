# Watcher lifecycle

Blueprint is sole owner of `blueprint-watch.mjs`, its process lifecycle, &
`~/.blueprint/watchman.pid`. Membrane never spawns, adopts, supervises, or writes
watcher state.

## Retirement

MBR-201's former Membrane-supervisor watcher policy is retired. Shipped
supervisor configuration is schema v2 & deliberately contains no
`watcherPolicy`. Blueprint manages its own watcher lifecycle; Membrane consumes
Blueprint only through published APIs.

## See also

- `docs/plans/orthic/SEAM-CONTRACT.md` — D-S09 watcher ownership.
