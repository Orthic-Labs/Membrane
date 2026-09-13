# Membrane — final architecture for implementation

**Date:** 2026-09-13. Replaces earlier versions of this plan, including scheduled-supervision instructions. Architecture follows [decisions 20–24](docs/architecture/adr/2026-09-12-context-system-decisions.md) & [execution lifecycle](docs/architecture/execution-lifecycle-boundary.md). GLM chooses files, coding approach & implementation sequence; preserve useful existing changes.

## Engine lifetime

| Hub | Harness accessing Membrane | Engine |
|---|---|---|
| On | Yes or no | On |
| Off | Yes | On |
| Off | No | Off after bounded drain |

Hub always starts/adopts & holds Membrane while Hub is on. Optional start-at-login launches Hub, which launches Membrane. Closing dashboard alone does not stop Hub if tray remains running.

A harness starts/adopts same installed engine when beginning Membrane access & releases ownership when access ends. An access session can span multiple requests; do not start/stop engine per Pull, Push or hook. A merely open chat without Membrane access is not an owner. CLI or standalone hook access uses bounded ownership of this same engine.

Each owner is tracked independently. Losing Hub must not stop an engine still held by a harness; losing a harness must not stop an engine held by Hub or another harness. Process loss or expired ownership must release abandoned access. Final owner loss drains work & stops engine plus its governed workers.

No independent engine login startup, periodic scheduled restart task, always-on service or ownerless supervisor. Remove existing mechanisms that violate this rule. Crash recovery may restart engine only while valid Hub/harness ownership remains.

## One runtime

At most one `membrane.exe` engine per OS user & canonical installed state. Concurrent startup attempts converge on that instance before subsystem initialization. No alternate port, store or engine generation may evade singleton ownership.

Engine owns one planner & shared subsystem services, storage handles, models, watchers & caches. Clients never initialize another Membrane runtime or open subsystem stores as an ordinary fallback. Engine handlers call shared services directly rather than forwarding through HTTP to themselves or spawning another Membrane executable.

Engine lifetime & background authorization are separate. Ordinary harness access does not grant watchers, repository enrollment or automatic work. Hub & eligible CodeRight ServiceHost background ownership retain their existing authorization rules.

## Harness connections

- **Codex & Claude Code:** direct authenticated Streamable HTTP MCP into shared engine. No persistent per-chat forwarding process.
- **CodeRight:** reuse existing typed authenticated HTTP integration with bounded connection reuse; retain existing Windows diagnostics pipe into same engine. ServiceHost & InlineHost acquire/release appropriate access ownership; background authority remains separate.
- **Stdio-only hosts:** lightweight `membrane-client` forwarding process per required stdio session. It owns transport only, with no planner, stores, models or runtime fallback.
- **Hooks & CLI:** route into same engine. Use supported direct host transport where available; command-only integrations may launch a small forwarding client. Containment helpers, where required, own no Membrane runtime or storage.

An HTTP URL cannot start a stopped engine. Supported harness integration must provide installed activation, ownership maintenance & release alongside direct HTTP requests. Connection pooling or an MCP session identifier alone is not lifetime ownership. Implement this through supported host lifecycle facilities; do not substitute an always-on daemon.

Reuse connections & service handles across requests. HTTP is selected for supported direct attachment & interoperability, not as a claim that it is intrinsically faster than native pipes.

## Shared execution & correctness

Blocking provider/storage work runs on bounded workers. Admission remains responsive, with fair capacity across chats, CLI, hooks & background work. Propagate deadlines & cancellation through queued & running work; slow Pull or indexing must not block unrelated chats or health checks.

Classify retry safety by actual operation, including hook events. Never silently replay a possibly completed mutation after transport failure; return an explicit uncertain-execution result unless safe deduplication proves retry cannot duplicate effects.

Preserve caller identity, grants, repository scope, session isolation, freshness & receipts across every transport. Push preserves submitted memory bytes through Cortex. Hook responses must match actual host schemas: unavailable injection reports typed degradation; applicable enabled enforcement returns a host-recognized block. Lifecycle activation failure must remain distinguishable from no matching context.

## Installation & completion

Use installer-owned stable `current` only. Preserve Hub login preference, migrate obsolete engine/task/stdio registrations & prevent old/new engines from owning same stores together. Readiness must distinguish a reachable engine from ongoing indexing; report degradation honestly with bounded waits.

Completion requires installed evidence for all lifetime-table states, overlapping owners, simultaneous startup, owner loss, crash recovery, direct HTTP hosts, stdio forwarding, hooks, Pull & Push. Verify exactly one runtime while owned, zero engine after final drain & no independent restart. Measure cold startup separately from warm calls at 1, 5 & 20 clients, idle & indexing, with equivalent inputs, grants & outputs.

Keep architecture requirements separate from implementation status. Update generated product truth only from verified behavior. GLM owns implementation details & should work through remaining defects without adding another architecture or approval phase.
