# Ripwire intake: owner-correct implementation and acceptance refinements

Date: 2026-09-07
Membrane audit baseline: `51ddd3f776807246e2666b15a43d77ce0e77f2ea`.
Donor: `redhat-et/ripwire` at `93c8edaafdb5499e89939cc2cebd0429e278e86f`.
Authority: explicit user request to incorporate useful lessons into the owning subsystem canons and push to main.

## Status and counting

This is a source-reviewed donor-intake decision, not implementation, benchmark, or release evidence. No donor code, dependency, daemon, parser, new public tool, or new capability atom is introduced. Existing capability rows and their lifecycle/competitive states remain unchanged. Existing qualification rows receive unverified acceptance refinements without changing their pending/stale states; new decision rows distinguish accepted refinements, exclusions, and HOLD experiments. Acceptance details live in the corresponding atomic canon's Ripwire intake section. They do not certify earlier evidence against newly added tests.

The baseline has 353 capability rows: 340 committed and 13 exploratory. Do not count qualification or decision rows as new capabilities. The unfinished `blueprint-completion` branch is not evidence of current-main delivery.

## Source register

All donor locators below are pinned to the donor revision above. They are design/code-reading evidence, not a claim that we executed the donor or reproduced its reported benchmarks.

- [Lexical retrieval](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/src/lexical.h): symbol-name subtokens plus callee/comment/body vocabulary; shared acronym-aware tokenizer; importance is not task relevance; conservative scoring/pruning equivalence.
- [Extraction/cache records](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/src/ingest_cache.h): cached raw per-file facts and lexical statistics, parser identity, selective record access.
- [Architecture and cache/determinism contract](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/docs/ARCHITECTURE.md): cold/warm equivalence; narrowed reads must not destroy wider cached coverage; ranking computation disclosure; approximate graph limitations.
- [Next operation](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/src/nextverb.h): one actionable follow-up with parser-level validation. Membrane adapts this to typed advisory operations, not auto-executed shell strings.
- [Test runner evidence](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/src/testmap.h): a named test file is not automatically a known runnable command.
- [Change-relative diagnostics](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/src/quality.h): baseline-relative regressions rather than absolute warning volume; structural metrics and heuristic thresholds are not compiler proof.
- [Task bundle](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/src/packtask.h): shared CLI/MCP assembly, progressive signatures/bodies, and accounting for wrapper/disclosure bytes. Membrane keeps assembly policy in Pull and representation/recovery in Push, not Blueprint.
- [Evaluation instruments](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/docs/EVALS.md): independently authored labels, repository-disjoint holdout, pollution/strict coverage metrics, counterexamples, and explicit cache-state caveats. Published donor numbers are not Membrane qualification.
- [MCP lifecycle](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/src/mcpindex.h): reviewed as a counterexample, not adopted; MCP-owned watcher/stat sweeps do not replace Hub-owned Blueprint residency and bounded reads.
- [LICENSE](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/LICENSE) and [third-party inventory](https://github.com/redhat-et/ripwire/blob/93c8edaafdb5499e89939cc2cebd0429e278e86f/THIRD_PARTY.md): the repository root is Apache-2.0; vendored material has independent notices. This intake copies no implementation. Any later source reuse needs exact-file licence review and preserved notices.

## Owner mapping

| Owner | Useful refinement | Already owned / not a new subsystem |
|---|---|---|
| Blueprint | Code-local lexical fields; cache lookup before payload hydration; incremental lexical statistics; typed next operation; comparable post-edit diagnostics and evidence-backed runner hints | BPT-014/019/020/021/023/028/034/037/041/042/044/049/051 |
| Ledger | Document-specific identifier/field qualification; exact-versus-literal distinction; cold/warm/narrow/wide projection coverage and resolver equivalence | LDG-002/005/006/007/008/010/011/017/018/022/027/029/030/031 already cover the document mechanisms |
| Pull | Evaluate task relevance separately from graph importance; source-owner composition; bounded corrective discovery and callable follow-ups; independent end-to-end retrieval metrics | PUL-002/004/012/015/016/017/018/019/020/021/022/023/031/042 |
| Push | Syntax-aligned detail alternatives under existing fidelity/recovery; measure the actual complete model-facing envelope; compare cost at equal task satisfaction | PSH-003/011/014/015/016/019/025/027/029 |
| Adapt | Consume version-bound regression diagnostics as diagnostic evidence only; matched exposure/outcomes and hard-negative qualification before learning claims | ADP-005/017/018/019/021/025/031/038/040/076 |
| Cortex | No new durable-memory mechanism established by this donor; document/field-note search is not governed memory admission | CTX-002/004/020/021/033 |
| Membrane | Whole-product independent code tracing and contract-first dogfood oracles; explicit evidence levels, lifecycle, and consumer reachability | MEM-001/002/008/009/013/016/024/035/038/040/042/043/047/054 |

## Boundary decisions

General Markdown/document retrieval stays in Ledger. A docstring/comment attached to a code entity is Blueprint-local search metadata, not document truth. Code/document reconciliation preserves both source identities and each owner's version. Pull composes owner evidence without giving a second copy independent corroboration. Neither subsystem opens another owner's private database.

Hub-off Blueprint remains bounded and on demand, with no watcher or replacement Membrane runtime. Operational Ledger/Pull/Cortex/Adapt services do not acquire Blueprint's one-shot exception. Live diagnostics remain a separate existing Membrane owner; Blueprint verification neither starts a second LSP fleet nor fabricates agreement from the absence of diagnostic errors.

## Exclusions and experiments

Do not adopt approximate name-matched edges as canonical facts, a fixed silent crawl denylist, sorted-index node IDs as stable semantic identity, MCP-owned continuous watching, editing authority, memory notes as durable truth, or a second context packer. Do not adopt donor constants, minified XML, broad tool counts, or compression headlines as requirements.

Personalized PageRank/graph diffusion, semantic retrieval, path demotion, adaptive cutoffs, and new compression formats remain experiment-only unless independently promoted under the existing owner. They must beat a frozen control on an independent workload at comparable cold/warm state and budget without weakening authority, freshness, source coverage, or task success. A donor result or a test written from the current output is not promotion evidence.

## Verification scope for this intake

Only canon/document checks and their Node regressions are applicable to this documentation change. No Rust/C++ build, product build, package, release, installed-host qualification, or donor benchmark is authorized or claimed. Separate subsequent semantic and dogfood audits must remain independent until their initial reports and test oracle are frozen.
