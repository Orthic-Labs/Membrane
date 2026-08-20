# Blueprint MCP reference

Blueprint MCP serves over stdio via `blueprint mcp serve --root <repo>` or `node scripts/blueprint-mcp.mjs --root <repo>` on Node ≥22.22.3 with SDK 1.x. Root binds at process start; every tool is read-only & every result is redacted.

## Contract

Surface is frozen at 6 tools, 9 resources (8 static plus `blueprint://effects`), & 6 prompts. Inputs are strict; every tool is read-only, idempotent, & closed-world. Successes return SDK-validated `structuredContent` plus identical redacted JSON text. Errors are stack-free `{ schemaVersion: 1, error: { code, message, details, retryable, remediation } }`; remediation is `{ summary, nextOperation, arguments }` or `null`. Common optional inputs are `repoId`, `generation`, & `allowStale`; unrestricted `repoRoot` is rejected.

## Effect profiles

`mcp/effects.mjs` exports a frozen `TOOL_EFFECTS` table with one profile per registered tool. The server exposes it as `blueprint://effects` & attaches it to each tool's metadata.

| Tool | reads | writes | executesProjectCode | network | installsSoftware | destructive | idempotent | approval |
|---|---|---|---|---|---|---|---|---|
| `blueprint_recall` | `["repository-graph"]` | `[]` | `false` | `none` | `false` | `false` | `true` | `not_required` |
| `blueprint_search` | `["repository-graph"]` | `[]` | `false` | `none` | `false` | `false` | `true` | `not_required` |
| `blueprint_expand` | `["repository-graph"]` | `[]` | `false` | `none` | `false` | `false` | `true` | `not_required` |
| `blueprint_impact` | `["repository-graph"]` | `[]` | `false` | `none` | `false` | `false` | `true` | `not_required` |
| `blueprint_doc_truth` | `["repository-graph", "repository-documents"]` | `[]` | `false` | `none` | `false` | `false` | `true` | `not_required` |
| `blueprint_status` | `["repository-graph"]` | `[]` | `false` | `none` | `false` | `false` | `true` | `not_required` |

Profiles are hints for hosts (AX P14); the RootRegistry and application service
remain the enforcement layer. Because every profile is read-only and
idempotent, no tool requires user approval to invoke.

## Tools

### blueprint_recall

- **Purpose:** establish the current repository and generation context before
  acting. Returns an admission decision (typically `action: "allow"`), the
  candidate context set for the task, and a freshness receipt.
- **Use when:** starting a session or task; deciding whether the graph is fresh
  enough to act on; scoping a task to a candidate set before reading code.
- **Do NOT use when:** you need a targeted query result list (`blueprint_search`),
  a bounded neighborhood around one anchor (`blueprint_expand`), or repository
  health/freshness state (`blueprint_status`).
- **Input fields:** `repoId?`, `generation?`, `allowStale?`, `task?`
  (string), `query?` (string), `limit?` (integer 1–100). At least one of
  `task`/`query` should be supplied; `query` is used for candidate retrieval,
  `task` for the admission decision.
- **Output schema:** admission decision per
  `schemas/blueprint-admission-v1.schema.json`
  (BlueprintAdmissionDecisionV1), with the embedded candidate set per
  `schemas/context-candidate-set-v1.schema.json` (ContextCandidateSetV1). The
  live envelope adds `generationId`, `freshnessReceipt`, and `omissions`, and
  carries a `claimBoundary` block when the CX-B3 capability is present
  (`{ status, cleanClaimAllowed, safeClaims[], prohibitedClaims[], gaps[] }`).
- **Effect profile:** reads `repository-graph`; all other effects false/none.

### blueprint_search

- **Purpose:** search symbols, files, claims, routes, and concepts in the
  current generation of the repository graph.
- **Use when:** finding definitions or references by name; building a focused
  query result list; the first step of a debug or review workflow.
- **Do NOT use when:** you need the neighborhood around one anchor
  (`blueprint_expand`), upstream impact of a change (`blueprint_impact`), document
  claim verdicts (`blueprint_doc_truth`), or an admission/candidate decision
  (`blueprint_recall`).
- **Input fields:** `repoId?`, `generation?`, `allowStale?`, `query`
  (string, required, minimum 1), `limit?` (integer 1–100).
- **Output schema:** adapter-registered inline schema (no shipped schema
  file). Live shape: `{ schemaVersion: 1, kind: "search", generationId,
  provider, query, results[], omissions[], truncated, continuationCursor,
  freshnessReceipt }`.
- **Effect profile:** reads `repository-graph`; all other effects false/none.

### blueprint_expand

- **Purpose:** expand one anchor (file path or symbol) into a bounded evidence
  slice of neighboring nodes and edges, with explicit budget bounds and
  pagination.
- **Use when:** you have one concrete anchor and need its neighborhood in both
  directions; continuing a large slice via `cursor`; staying inside a token
  `budget`.
- **Do NOT use when:** you need upstream dependents and impact analysis
  (`blueprint_impact`), or a keyword search (`blueprint_search`).
- **Input fields:** `repoId?`, `generation?`, `allowStale?`, `anchor`
  (string, required, minimum 1; `file:`/`symbol:` prefixes are honored),
  `depth?` (integer 1–5), `budget?` (integer 128–32000), `cursor?` (string).
- **Output schema:** canonical subgraph slice per
  `schemas/repository-subgraph-slice-v1.schema.json`
  (RepositorySubgraphSliceV1). Live payload: `kind: "neighbors"` with
  `nodes[]`, `edges[]`, `counts`, `truncated`, `continuationCursor`,
  `budget`, and freshness fields.
- **Effect profile:** reads `repository-graph`; all other effects false/none.

### blueprint_impact

- **Purpose:** return the upstream impact of one anchor — dependents, affected
  tests, routes, and schemas — plus uncertainty/omissions, within a bounded
  slice.
- **Use when:** planning or reviewing a change; assessing what a symbol/file
  touches before editing; continuing a large impact slice via `cursor`.
- **Do NOT use when:** you need an undirected neighborhood (`blueprint_expand`),
  a keyword search (`blueprint_search`), or repo health (`blueprint_status`).
- **Input fields:** `repoId?`, `generation?`, `allowStale?`, `anchor`
  (string, required, minimum 1), `depth?` (integer 1–8), `budget?` (integer
  128–32000), `cursor?` (string).
- **Output schema:** canonical subgraph slice per
  `schemas/repository-subgraph-slice-v1.schema.json`
  (RepositorySubgraphSliceV1). Live payload: `kind: "impact"` with `target`,
  `impacted[]`, `nodes[]`, `edges[]`, `counts`, `truncated`,
  `continuationCursor`, `budget`, and freshness fields.
- **Effect profile:** reads `repository-graph`; all other effects false/none.

### blueprint_doc_truth

- **Purpose:** return document claims with their current truth status —
  current, stale, contradicted, or unknown — against the code graph.
- **Use when:** reconciling documentation with code; verifying whether a doc
  claim is supported before quoting it; the documentation-reconciliation
  workflow.
- **Do NOT use when:** you need code symbols or references (`blueprint_search`,
  `blueprint_expand`), or repo health (`blueprint_status`).
- **Input fields:** `repoId?`, `generation?`, `allowStale?`, `claimId?`
  (string), `kind?` (string), `limit?` (integer 1–1000).
- **Output schema:** envelope
  `{ schemaVersion: 1, generationId, claims[], freshnessReceipt, omissions,
  truncated }`; each claim entry validates against
  `schemas/document-claim-v1.schema.json` (DocumentClaimV1: `id`,
  `documentId`, `source`, `line`, `status`, `sha1`, `edges`).
- **Effect profile:** reads `repository-graph` **and** `repository-documents`;
  all other effects false/none. This is the only tool whose `reads` includes
  documents.

### blueprint_status

- **Purpose:** return freshness, coverage, service health, and repair actions for the selected repository graph.
- **Use when:** checking whether a graph exists and is fresh before trusting results; after a build; diagnosing a missing/corrupt/stale store.
- **Do NOT use when:** you need context or candidates (`blueprint_recall`), a
  query (`blueprint_search`), or claim truth (`blueprint_doc_truth`). Status reports
  repair actions; it does not perform them (build/repair are CLI operations).
- **Input fields:** `repoId?`, `generation?`, `allowStale?` — common fields
  only.
- **Output schema:** `schemas/repository-status-v1.schema.json`
  (RepositoryStatusV1). Live envelope adds `repository` identity and the
  graph-status fields (`state`, `manifestPath`, `manifest`,
  `providerMismatch`, `manifestDigestValid`, `ledger`, `pendingPaths`,
  `scanTruncated`, `truncationReasons`, `clocks`, `capabilities`), plus a
  `claimBoundary` block when the CX-B3 capability is present.
- **Effect profile:** reads `repository-graph`; all other effects false/none.
## Error codes

Stable error codes surfaced in the typed error envelope, with the CX-B3
declared `retryable` / `remediation` mapping where one exists. Codes not listed
in the CX-B3 mapping carry the envelope defaults (`retryable: false`,
`remediation: null`); the operator action below still applies.

| Code | Raised when | `retryable` | Envelope remediation | Operator action |
|---|---|---|---|---|
| `stale_blocked` | Freshness barrier did not catch up and `allowStale` was falsy | `true` | `blueprint_recall` with `{ allowStale: true }` | Recall to the current generation, or re-run the query with `allowStale: true` |
| `generation_mismatch` | Requested `generation` is not the current sealed generation | `true` | Recall (`blueprint_recall`) to obtain the current `generationId` | Recall, then retry with the current generation |
| `root_not_enrolled` | `repoId` (or `repoRoot`) does not match an enrolled root | `false` | Names enrollment | Start the server with `--root`/`--repo-id`/`BLUEPRINT_REPO_ROOTS` covering the repository |
| `root_escape` | `repoId` and `repoRoot` resolve to different enrollments | `false` | — | Pass only one selector; never combine mismatched ones |
| `graph_missing` | No `.agent/graph/graph.db`, or no sealed generation | `false` | — | Build the graph (`blueprint build`) and retry |
| `anchor_not_found` | Anchor matched no graph node | `false` | — | Use a more specific anchor (qualified symbol or `file:` path) |
| `anchor_ambiguous` | Anchor matched more than one node; candidates are in `details` | `false` | — | Disambiguate using `details.candidates` |
| `barrier_timeout` | Freshness sync exceeded its timeout | `false` | — | Retry the request; if persistent, check the repository for a stalled watcher/build |
| `request_cancelled` | Request aborted by the caller/signal | `false` | — | Retry if still needed |
| `resource_not_found` | Unknown resource URI requested | `false` | — | Use one of the documented `blueprint://` URIs |
| `internal_error` | Fallback when the thrown error carries no code | `false` | — | Report the message from the envelope |

Never trust a result or error that carries a stack trace: the adapter strips
stacks and redacts `details` before egress.

## Resources & prompts
Static resources are `manifest`, `languages`, `providers`, `architecture`, `claims`, `conflicts`, `rules`, & `receipts`; `blueprint://effects` exposes `TOOL_EFFECTS`. Prompts are `recall-before-change`, `debug`, `review`, `architecture-validation`, `documentation-reconciliation`, & `onboarding`; they reference tools, never repository prose.
