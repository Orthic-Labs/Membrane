# Blueprint federation contract

Each candidate set and repository neighborhood is independently scoped by `repoId`, `repoRoot`, and `receiptId`. `repoId` is `xxh128` over normalized repository root plus configured Git `origin` URL when present.

`blueprint-watch barrier-all --json` fans out PR5 freshness barriers across every enrolled repository with `Promise.all`; each receipt remains independent, so one repository's `event_gap` or timeout does not block another repository's result.

Raw cross-repository graph merging is rejected by design. Membrane composes independent scoped candidate sets and neighborhoods; Blueprint does not create a synthetic cross-repository graph.
