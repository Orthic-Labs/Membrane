# Donor lifecycle & cancellation review

2026-09-14. Twelve Luna workers screened five corpus entries each. Primary reviewed Membrane paths & checked shortlisted upstream evidence. This is source research, not installed qualification or a performance benchmark. Local donor clones were absent; upstream heads may differ from historical donor snapshots. README-only rows establish no execution guarantee; plain MemoryOS identity remains unresolved.

## Finding

HTTP is not established as this failure's cause. Membrane's synchronous compatibility bridge creates a per-Pull Tokio runtime, while HTTP dispatch already runs inside a resident runtime. Pull creates a fresh token rather than inheriting HTTP cancellation. A timeout can therefore return while underlying work retains its admission slot.

Source paths:

- `engine/crates/membrane-runtime/src/mcp_http.rs:378`: bounded MCP admission; permit moves inside blocking dispatch; timeout cancels request token.
- `engine/crates/membrane-runtime/src/mcp_executor.rs:1674`: Pull invokes synchronous federation wrapper without inherited request control.
- `engine/crates/membrane-runtime/src/pull/federation.rs:477`: comment explains bridge was added to avoid nested-runtime panic; scoped thread creates runtime & fresh cancellation token, then joins.
- `engine/crates/membrane-runtime/src/pull/native_federation.rs:28`: each runtime has two async workers & ten blocking workers. These limits are per runtime, not one shared engine-wide provider limit.
- `engine/crates/membrane-runtime/src/pull/native_federation.rs:339`: blocking provider wrapper exists because owner adapters perform synchronous filesystem, SQLite & Blueprint work. Moving those calls directly onto scheduler threads would restore timer starvation.
- `engine/crates/membrane-runtime/src/serve.rs:6684`: uninstalled source bounds runtime shutdown. This stops waiting; it does not terminate running blocking work.

[Tokio's contract](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) explicitly says started blocking tasks cannot be aborted; ordinary runtime shutdown waits for them. `shutdown_timeout` limits that wait while tasks may continue. This validates a failure mechanism, not proof that every observed installed hang shares one cause.

## Useful mechanisms, qualified by evidence

| Donor | Useful mechanism | Limit / disposition |
|---|---|---|
| [Mnemon daemon source](https://github.com/mnemon-dev/mnemon/blob/master/internal/daemon/daemon.go) | Stop admission, cancel shared context, shut server, join requests, close writer. | Primary verified `closeOwned` has unbounded request join after its two-second server budget. Keepalives disabled. Useful ordering; not a complete bounded-shutdown solution or HTTP performance winner. |
| [Haystack pipeline source](https://github.com/deepset-ai/haystack/blob/main/haystack/core/pipeline/pipeline.py) | `PipelineStreamHandle` owns pipeline task, cancels on abandonment, attempts cleanup with a one-second timeout. | Primary verified task ownership/cleanup. Coroutine cancellation is not termination of synchronous worker threads; a cleanup timeout is not a universal hard process-exit guarantee. |
| [MemOS HTTP helper](https://github.com/MemTensor/MemOS/blob/main/apps/memos-local-openclaw/src/client/hub.ts) | `hubRequestJson` combines caller signal & timeout signal, passes resulting signal to `fetch`. | Primary verified helper. This proves network-operation cancellation composition, not all server-side work cancellation; public search helper does not itself expose caller signal. |
| [Caura timeout middleware](https://github.com/caura-ai/caura/blob/main/core-api/src/core_api/middleware/request_timeout.py) | Explicit distinction between response timeout & ongoing blocking provider work. | Confirms same class of trap; not a cure by itself. |
| [Codebase-memory-mcp application](https://github.com/DeusData/codebase-memory-mcp/blob/main/src/daemon/application.c) & [runtime](https://github.com/DeusData/codebase-memory-mcp/blob/main/src/daemon/runtime.c) | Strongest reviewed match: request-token cancellation, session cancellation, job subscriber ownership & final-client shutdown for non-permanent generations. Primary verified relevant symbols. | Runtime contains bounded unresponsive-handler containment that can terminate whole process, not magically cancel a thread. Do not copy permanent-generation mode, local CLI runtime duplication or routine worker subprocesses. |
| [Honcho async client](https://github.com/plastic-labs/honcho/blob/main/sdks/python/src/honcho/http/async_client.py) | Worker found reusable client, explicit owned/borrowed client distinction, close & bounded network retries. | Useful transport ownership; does not prove server worker reclamation after request cancellation. |

Initial worker ranking of OpenViking as best daemon shutdown was rejected. Its cited watchdog/drain matrix describes host integrations, notably Hermes, rather than OpenViking server. [Integration reference](https://github.com/volcengine/OpenViking/blob/main/docs/en/agent-integrations/16-capability-reference.md). No donor demonstrated a complete, benchmarked replacement for Membrane's required lifecycle. Codebase-memory-mcp is strongest source-level lifecycle/cancellation reference among this screen; Haystack & MemOS supply narrower task/transport patterns. Upstream raw responses showed changing line counts, so use named symbols rather than treating worker line anchors as pinned revisions. Donor tests were inspected selectively, never executed.

## Recommended correction

1. Keep one Hub/harness-owned installed engine. Final owner loss stops admission, signals cancellation, drains tracked work, closes stores safely & exits. No independent startup/restart task.
2. Execute resident Pull on engine-owned async execution resources. Remove per-request runtime construction from resident path; bridge legacy synchronous callers into shared execution without blocking scheduler threads.
3. Carry one absolute request deadline & linked cancellation through dispatch, federation & provider adapters. No fresh unrelated token or reset timeout at an inner boundary. Owner loss cancels descendant work too.
4. Keep unavoidable synchronous work in an engine-wide bounded executor. Check cancellation before starting & between bounded units; give blocking I/O/locks their own remaining-deadline limits. Cancellation must reach work, not just its waiter.
5. Retain work permits until work actually exits. Preserve bounded admission & ensure Pull cannot exhaust capacity needed by Push/control work. Do not solve retention by increasing limits or releasing permits while untracked work continues.
6. Prove with controlled slow providers: request cancellation reaches provider; queued expired work never starts; slots recover; Push/health remain responsive; final owner loss reaches process exit. Test actual store integrity after shutdown. Then installed qualification on a separately authorized candidate; original three-build allowance is exhausted.

No process-per-Pull architecture, extra daemon, replacement HTTP transport, new cache system or unconditional force-kill policy follows from this review.

## All 60 corpus entries screened

These are bounded worker dispositions, not 60 fully audited repositories. "No proof" means reviewed material did not establish relevant behavior, not that code cannot contain it. Primary acceptance is limited to findings above.

| Batch | Five entries | Relevant outcome |
|---|---|---|
| 1 | Mnemon; OpenViking; Hindsight; memvid; rtk | Mnemon ordering useful with unbounded join; OpenViking integration/server conflation corrected; no accepted complete cancellation solution from remaining three. |
| 2 | graphiti; zep; mem0; letta; honcho | Shared client ownership, coroutine queues & close semantics; server cancellation guarantees not established. Current SDK/client evidence sometimes replaces archived server surface. |
| 3 | haystack; langchain; llama_index; txtai; graphrag | Haystack task ownership useful; concurrency knobs & thread bridges do not terminate running synchronous work. |
| 4 | MemOS; MemoryOS; MemoryOS-bailab; langmem; cognee | MemOS signal composition; LangMem pending-work cancellation; Cognee bounded queue. Plain MemoryOS identity unresolved; not counted as verified upstream. |
| 5 | codebase-memory-mcp; context-mode; superlocalmemory; mnemosyne; memory-lancedb-pro | Codebase-memory-mcp lifecycle/cancellation code verified after initial README screen. Subprocess containment in context-mode is not permission to spawn a process per ordinary Pull. Remaining memory algorithms irrelevant to failure. |
| 6 | cline; byterover-cli; claude-subconscious; emulo; headroom | Cancellation APIs & proxy lifetimes; detached workers are not final-owner guarantees. Cline persistent Hub semantics differ from required final-owner stop. |
| 7 | archive-old-cli-mentat; codealmanac; brain0; treesitter-chunker; Ivy-Tendril | Codealmanac cancellation/confirmed process termination is relevant only for containment. Others offer basic lifecycle/admission or no complete proof. |
| 8 | repo-graph; baseai; prpack; code-compress; PraisonAI | Serialization, host stop & cancellation API examples; none proves cancellation reclaims blocking work. |
| 9 | rasa; Supercompress; synalinks; caura; CodeGraph | HTTP abort propagation & deterministic transport closure useful. Rasa/Caura expose blocking-thread trap; CodeGraph queue/final persistence do not prove bounded shutdown. |
| 10 | rag-rat; claude-token-efficient; greplica; code-review-graph-rescript; Memary | No source-backed complete lifecycle/cancellation mechanism accepted; broad retrieval/freshness claims excluded. |
| 11 | bondai; mengram; context8; agentmemory; semantic | README-level ownership/queue claims or unrelated parser/storage behavior; no accepted timeout-reclamation solution. |
| 12 | semantica; memonto; supermemory; vanna; lean-ctx | Predominantly product/API documentation; no accepted bounded daemon/Pull cancellation proof. |

Research only: no product changes, builds, installation or runtime launches performed for this review.
