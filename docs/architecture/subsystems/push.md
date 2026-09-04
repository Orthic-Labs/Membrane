# Push: verified preparation, recovery and measured delivery

Implementation branch: `push-end-to-end`, based on `067892355900613a3fde9685e25186c2de7ccbb8`. Design authority: the user-approved 5 September 2026 Push implementation plan and revised atomic canon. The canon is `docs/canon/push.md`; implementation evidence and release qualification are separate. This document describes the code contract, not a claim that every host or all 29 release gates are qualified.

## Ownership

Push changes the representation of already-authorized evidence. Pull/Membrane retains source admission, authority, ranking, membership and final policy. Ledger remains the governed document/source owner. Cortex is not a transient capture database. Blueprint owns symbol resolution; Push's source projection only removes parsed function bodies. The resident service never executes commands.

The shared owner is `push::delivery::prepare`; `push::recovery::RecoveryStore` retains immutable authorized originals. Native MCP, resident HTTP and CLI delegate to those owners. The opt-in JavaScript tool-egress middleware processes an already-executed result, never runs the command again, and does not claim interception of arbitrary external tools.

## Agent surface

Negotiate the `push` group in `tools/list` using `_meta: {"membrane.toolsets.v1":["push"]}`. Default discovery remains `membrane_context`. The added tools are `membrane_push_prepare` and `membrane_push_resolve`.

First call `membrane_push_resolve` with `operation: "probe"`, `repository`, and `caller: {root, repositoryId, scopeId}`. A successful authorized probe returns a short-lived `resolverToken` bound to the same scope and store. Pass that token in a preparation request. A caller boolean is not recovery proof. Restart invalidates the token; retained originals remain resolvable. Reads never renew an artifact's expiry.

```json
{
  "repository": "repository-id",
  "caller": {"root":"/authorized/repository","repositoryId":"repository-id","scopeId":"session-scope"},
  "request": {
    "text":"already-authorized tool output",
    "kind":"text",
    "maxBytes":16000,
    "resolverToken":"token returned by probe",
    "optimize":true,
    "exact":false
  }
}
```

`maxBytes` is the serialized Push delivery budget, not a model-token observation. Kinds are `text`, `code`, `json`, and `log`. Code accepts `sourcePath` for classification only; this API does not read an arbitrary path. Caller-supplied protected byte spans can add obligations, never grant access.

Resident aliases are `POST /push/prepare` and `POST /push/resolve`. `/expand` now delegates to the same scope-authorized resolver. The old unscoped request shape is deliberately refused. Legacy `/compress` retains its `out` field but returns exact input with `scoped_push_prepare_required_for_reduction`; use scoped preparation to authorize retention and reduction.

Recovery supports whole bounded originals, zero-based half-open byte ranges, one-based inclusive line ranges, and exact JSON field/index/string-key navigation. Line endings and numeric spellings are retained. JSON strings return their original quoted JSON bytes. Duplicate keys, ambiguous record matches, invalid selectors and excessive depth/work are refused, not first-match guesses. Binary bytes use explicit hex transport. Every success identifies the parent and slice digests and has `disposition: "exact"`.

## Preparation and validation

A reduction is eligible only after source/scope/capability checks. Original bytes are committed and read-back verified before a recovery marker is issued. A content digest without a stored original is not presented as a recovery handle.

The independent validator compares source/output byte spans to an immutable original digest. It verifies occurrence identity, ordering and mandatory-span coverage. It does not build a validation universe from the edited output. Generated AST elision markers and named structured codecs are distinct from verbatim source spans; neither extractiveness nor a retained disk copy is a universal semantic-fidelity proof.

The conservative text path extracts whole original lines and preserves recognized constraints, errors, negation, numeric/path evidence and fenced blocks. The AST path uses parsed body fields for Rust, Python, JavaScript, TypeScript and TSX; multiline signatures, destructuring, decorators, imports, exports and data fields remain. Unsupported/error-containing parses stay exact. JSON whitespace compaction preserves source lexemes and rejects ambiguity. Repetitive log folding is decoded and compared with the exact original before admission.

Exact/refused results cannot silently trigger another lossy fallback. If protected or exact content cannot fit, the owner returns a typed refusal. The legacy batch `prep` interface retains its explicitly labeled lexical-unit budget for compatibility; its manifest records actual output and budget-met state. It is not a substitute for a model-token or final-wire capacity proof. Preparation preloads bounded originals before writes and refuses output aliases of any source.

## Measurement

Packet plans are materialized before measurement. The registered counter is exact `o200k_base/1`, using literal-data encoding rather than treating token-looking source strings as special tokens. Unknown estimator bases are refused, not relabeled. Full/reduced/floor preserve evidence identity and protected obligations. Without a consumer-proven resolver, the ladder cannot invent a recoverable lossy representation.

Native MCP additionally measures its actual serialized tool-result shape, including text and structured metadata. It reuses the existing ladder to choose a fitting candidate without rerunning providers or inventing a new H8 observation. The final measurement is separate from the packet selection count. The JavaScript owned boundary measures the complete tool-result envelope and verifies the returned source and representation digests. Unselected original/alternate bodies are not injected as diagnostic data.

These scopes are explicit. Neither serialized packet bytes nor MCP tool-result tokens prove provider billing or account for unobserved host messages. The host must supply genuine remaining capacity and reserve its own framing. Optional compression requires positive measured savings; required safety caps and absent economics are not mislabeled savings.

## Capture, storage and lifetime

CLI `runc` uses validated direct argv by default. Legacy shell syntax requires explicit `--shell` with one quoted command string; it is not a remote execution endpoint. Capture executes once, preserves status and stream observations, and bounds duration, queued chunks and total bytes. Cancellation/limit failures never publish an incomplete original as exact. Lossy UTF-8 previews retain a byte-exact original; small valid text is passed through without a dangling anchor.

The shared store is `MEMBRANE_ANCHOR_DIR` or `<workspace>/tools/.cache/runc`. CLI scope uses `MEMBRANE_PUSH_SESSION` (default `local`); the canonical root and session must match a resolver's caller binding. CLI `push restore` supports `--selector` JSON and `--max-bytes`. CLI `push prepare` accepts a JSON request via file or stdin. Explicit `push lease` renewal uses expected-expiry compare-and-swap; invalidation is separate and never implicit.

SQLite commits payload and metadata atomically. Object reuse verifies content, size and digest. The implementation bounds an original to 16 MiB, a restore to 256 KiB, per-scope retained payload to 64 MiB, total logical payload to 256 MiB, object rows to 4,096 and database pages to 512 MiB. Native response framing imposes an additional bound; a requested maximum is not permission to overflow it. TTL is at most seven days. Unknown, corrupt, expired, invalidated, unauthorized and oversized results are distinct errors. Reads and repeated publication do not silently renew leases or resurrect expired handles.

Tombstones intentionally remain bounded rather than silently resurrecting old content-addressed references. Long-running retention maintenance, cross-platform process containment and installed-host behavior require their own operational qualification. Existing spill exports are compatibility artifacts, not authorization to bypass the resolver.

## Adoption and diagnostics

The JavaScript Membrane-owned result boundary is opt-in through `_meta["membrane.push.v1"]`, carrying a resolver token and byte budget. It preserves call IDs, error state and trace fields, excludes exact source reads and diagnostics, and leaves unsupported multimodal shapes outside lossy processing. Installation of Membrane alone is not proof of arbitrary host interception.

Push telemetry is optional, content-free, explicitly unit-bearing and bounded. It records process-local observation coverage and sink misses; absence is not observed zero. Provider-billed tokens and task outcomes remain unknown until an authorized external observer supplies them. Current query-aware caller opt-in does not manufacture authority/freshness: unproven policies remain terminal exact refusals. The existing feature-gated LLMLingua backend is retained, not presented as a new algorithm or exact artifact resolver.

## Validation and release gate

Focused checks cover scoped publication, read-back integrity, binary/CRLF restoration, expiry, invalidation, explicit renewal, exact selectors, source-span validation, AST headers, bounded command capture, source-safe batch preparation, materialized packet sizing and final MCP overhead. `tests/push_end_to_end.rs` exercises normal native discovery/dispatch, authenticated-scope owner checks, HTTP route dispatch, CLI restoration, reopening the store, tampering, expiry and registry revocation in a disposable binding.

That route-level test is not a claim of successful installation in Claude Code, Codex, CodeRight or a customer's machine. Full Windows/macOS installation qualification, learned/query-aware held-out quality, provider-usage joins and broad host interception must remain pending until measured. No release gate is closed merely because this branch compiles or a new unit test passes.
