# Ledger Markdown Indexing and Document Navigation Canon

**Revision date:** 2026-09-05
**Status:** selected architecture with source implementation on `ledger-end-to-end`; release qualification and installed-host verification remain pending
**Audit baseline:** `75c257ad711d19ffce69258d132a45dbffa9b4ac`
**Implementation branch:** `ledger-end-to-end` at `62243a9c099b53e5ef1694739aec8b9ca277b055` (source/type-check evidence; not a release receipt)
**Intended path:** `docs/architecture/subsystems/ledger.md`
**Supersession:** replaces the 2026-08-25 edition at this path when adopted, retaining its ownership boundaries, naming migration, source-authority model and qualification discipline
**Atomic state:** `docs/canon/ledger.md`
**Parent system:** Membrane
**Scope:** granted document registration, structural projections, source-local search, exact resolution, navigation, conversion, lifecycle, qualification and delivery

## Executive decision

Complete Ledger as a daemon-owned, source-bound document service reachable through the normal harness path. Retain its existing SQLite/FTS5 and structural mechanisms. Correct identity, lifecycle, complete-projection and resolver defects before enabling native document-provider delivery.

The six named Membrane subsystems remain Pull, Push, Cortex, Blueprint, Ledger and Adapt. Ledger replaces Guide; it is not a seventh subsystem. Source bytes remain authoritative with their source owner. Ledger indexes are rebuildable projections, not repository truth or durable semantic memory.

This revision strengthens seventeen existing atomic contracts and adds three distinct observable capabilities: **LDG-029 inbound references/link health; LDG-030 literal source-span matching; LDG-031 structural drift diagnostics**. The revised atomic inventory is 31 rows: 30 committed scope and one exploratory. Source implementations for the three new atoms are present on the implementation branch; verification, qualification and release delivery remain pending. **LDG-023 and Cortex CTX-033 remain exploratory/HOLD.** The three additions do not require a new search engine, graph database, protocol authority, resident process or Ledger crate.

Normative statements below define the selected target. Section 5 is the separate commit-pinned implementation diagnosis. Writing this canon does not turn its target diagram into current behavior.

## Runtime lifecycle binding — normative

Membrane runtime exists only in the headless child daemon of the visible native tray, with OS-enforced lifetime coupling. CodeRight binds to that active daemon; there is no embedded CodeRight Membrane backend.

Operational MCP, CLI and host surfaces are stateless authenticated clients. They MUST NOT open operational Ledger storage, start a replacement runtime, silently fall back to local index execution or create an index when reporting inactive status. Tray off returns the canonical typed `membrane_unavailable { reason: hub_inactive, retryable: true }` response. An explicitly retained offline developer utility must be separately named and excluded from normal product context behavior.

Blueprint remains independently usable but not independently resident. Its bounded one-shot exception is not permission for a resident or fallback Ledger service. All continuous Ledger update work uses the existing daemon's scheduling and cancellation mechanisms.

---

# 1. Supersession and interpretation

The Guide-to-Ledger rename and the earlier rejection of whole-document substring scanning as a finished retrieval design remain in force. This revision does not repeat already completed implementation work merely because an older diagnosis described it as missing.

The older current-state descriptions of ASCII-only queries, absent FTS execution and absent persisted links are historical. The audited revision contains Unicode/identifier processing, operative weighted FTS, typed block nodes and source-derived link expansion. Their existence is not evidence of complete source coverage, safe lifecycle reconciliation or delivery through the agent.

The final improvement plan is a delivery document; this file owns architecture and the atomic canon owns capability state. Historical audit/comparison/qualification records are retained at their original revisions. Revised contracts need their own applicable evidence rather than silently inheriting a narrower successful result.

# 2. Naming and migration contract

## 2.1 Canonical names

Use Ledger, `ledger`, `membrane_runtime::ledger`, `LedgerDb`, `ledger-index.sqlite3` and Ledger-labeled product diagnostics. Public command spelling must follow the actual dispatcher; the inspected executable uses `membrane cli ledger ...`. Documentation must not advertise an unimplemented shorter alias.

Guide and Spine remain only in historical documents, compatibility tests, rename records and bounded upgrade diagnostics. Existing storage may be explicitly retired/rebuilt or migrated; no new operational store may prefer the old names. A legacy file retained for rollback is included in retention and erasure accounting.

## 2.2 Session projection naming

Keep `ledger::session_projection`, not `ledger::ledger`. Preserve the earlier migration mappings where old serialized forms still require bounded compatibility:

```text
SessionLedgerInputV1       -> SessionDocumentProjectionInputV1
SessionLedgerDocumentV1    -> SessionDocumentProjectionV1
LedgerSourceCursor        -> SessionProjectionSourceCursor
LedgerEventV1              -> SessionProjectionEventV1
LedgerTaskV1               -> SessionProjectionTaskV1
LedgerArtifactV1           -> SessionProjectionArtifactV1
LedgerDecisionV1           -> SessionProjectionDecisionV1
build_session_ledger       -> build_session_projection
index_session_ledger       -> index_session_projection
```

Do not revive old names as parallel APIs. User-facing prose may describe a human-readable artifact as a session ledger without creating another code owner.

## 2.3 Migration coverage and rollback

Rename coverage includes architecture, generated truth sources, subsystem enums, CLI/MCP/host capability names where exposed, tests, schemas, fixtures, telemetry, installer diagnostics, Hub read models and cross-subsystem references. Regenerate generated files rather than hand-editing them.

Retire incompatible projections explicitly. Rebuild from canonical eligible sources; refuse unavailable source sets rather than fabricate a successful migration. Keep the previous compatible retrieval path for the qualified rollback window. Do not delete legacy state or remove `legacy_scan` solely because this revision was authored. Verify upgrade, migration, activation and rollback on the exact supported package before retirement.

# 3. Canonical ownership

## 3.1 Ledger owns

Ledger owns the grant-bound document registry, source/revision/hash metadata, source-positioned GFM document/section/block projections, logical/version/alias identity, source-derived links, source-local FTS and literal matching, bounded hierarchy/backlink navigation, exact source resolution, conversion projections, transactional publication, incremental reconciliation, tombstones, scoped erasure and diagnostic structural deltas.

It also owns source-local provenance, coverage and omission accounting. It materializes candidates for Pull; it does not decide final cross-provider admission.

## 3.2 Other owners remain authoritative

Blueprint owns repository semantics, source identity/truth services and code re-anchoring. Cortex owns admitted durable knowledge, conflicts, supersession and memory lifecycle. Adapt emits proposals rather than rewriting durable truth or Ledger policy. Pull/Membrane owns final eligibility, authority/freshness/sufficiency, fusion, context budgets and publication. Push owns faithful reduction after selection. CodeRight owns agent execution and host observations.

Ledger never opens Blueprint SQLite or Cortex durable storage. Integrations use typed owner handles. Shared source references do not authorize cross-owner database access. A `RuntimeDeliveryLedger` used by a rules provider is not the document subsystem.

## 3.3 Registry, caches and source authority

“Ledger holds all documents” means it registers and structurally indexes all eligible sources covered by a declared source manifest and grant. It does not own authoritative filesystem, Git or external-source bytes.

Caches must bind raw and projection hashes, source identity/revision, converter/parser versions and deterministic invalidation. Current-source claims require freshness evidence from the source owner. An immutable imported snapshot is explicitly labeled a snapshot; the presence of cached bytes does not make it a live external-source view.

Document identity is not content equality. Separate sources may share a physical content-addressed blob while preserving different grants, lifecycles and citations. Rebuild reproduces current projections; historical alias or drift claims require retained source-backed history. Lost history is reported as unavailable, not reconstructed as fact from similar text.

# 4. Locked invariants

1. Canonical source identity/revision and raw bytes remain authoritative; Ledger projections are rebuildable.
2. Ledger candidates are document-navigation evidence, never repository truth or durable-memory authority.
3. Scope, path, lifecycle, trust, influence and sensitivity eligibility precede ranking and expansion and are rechecked before byte release.
4. Pull/Membrane retains final fusion, admission, sufficiency, budgeting, publication and receipt authority.
5. A returned node binds source identity, expected revision/hash, supported span/range/hash, publication generation and parser/projection/query configuration.
6. Changed source, converter or span evidence never yields silently substituted bytes.
7. Document hierarchy and links are source-derived. Generic semantic relation inference is outside Ledger.
8. Expansion, enumeration, conversion, loading and reads obey inherited deadlines, cancellation and explicit work/output bounds.
9. A request carries a coherent publication/source tuple; concurrent change yields an explicit retry/refusal where coherence cannot be maintained.
10. Mixed-generation artifacts, nodes, links or FTS rows are not a publishable complete result.
11. Ledger owns its FTS state and never reuses Cortex storage.
12. Ledger never directly opens Blueprint storage.
13. No vector/embedding store is introduced by this revision.
14. A different search engine or semantic lane requires a measured deficiency and a separate architecture decision.
15. Unknown parser/schema/tokenizer/converter versions cause typed degradation, refusal or rebuild.
16. Corrupt projection state is rebuilt or refused, not served as verified evidence.
17. Generated session documents remain non-recallable until consumer, source authority, lifecycle, privacy and replay value qualify.
18. Repository or model text cannot grant authority, invent source identity or manufacture verified links.
19. An index on disk and an implemented helper are not proof of shipped behavior.
20. Production-path reachability and applicable acceptance evidence are required for capability closure.
21. API presentation pagination never silently truncates the internal index.
22. Equal bytes never merge distinct live document identities or authorization scopes.
23. A source collection may be tombstoned only by its authoritative, complete reconciliation; temporary observation failure is not deletion.
24. Every emitted resolver-backed reference is usable through the recipient host's advertised/negotiated operations.
25. Operational CLI/MCP calls use the daemon owner; inactive status creates neither storage nor runtime.
26. Search completeness and relevance are separate. Partial search cannot establish complete no-answer.
27. Literal-match claims require exact verification against the named source/projection bytes; FTS quoting is not such proof.
28. Backlink counts measure references within declared coverage, not truth, independent corroboration or higher authority.
29. Structural drift is diagnostic; change magnitude or low edit frequency is not an automatic ranking or truth rule.
30. Indexing must not insert block IDs into source text or rewrite source references. Author-provided IDs may be consumed as aliases.
31. Engine, provider, transport and host-delivery qualification are separate and release/configuration-bound.
32. Automatically marked pending work has an automatic completion path or typed terminal failure; it cannot remain dependent on an unrelated manual seal.

# 5. Current implementation reconciliation

Audit baseline: `75c257ad711d19ffce69258d132a45dbffa9b4ac`. Implementation source reviewed through `62243a9c099b53e5ef1694739aec8b9ca277b055` on `ledger-end-to-end`. Managed branch check `33952026427` passed the atomic-canon/source checks and `cargo check --manifest-path engine/Cargo.toml --workspace --tests --locked`. That type-checks library, binary and test source; it does **not** execute Rust tests, package/install a build, activate automatic Ledger delivery or establish release qualification.

| Boundary | Source state on implementation branch | Remaining closure |
|---|---|---|
| Storage and search | Repository-scoped exact metadata/identifier routing, weighted FTS5 machinery, explicit literal-span verification, source eligibility before rank, and bounded graph alternatives are in the daemon owner path. | No-answer/relevance corpus qualification and release-compatible provider activation remain pending. |
| Projection | Complete internal outline is independent of presentation paging; one Comrak AST feeds section, typed-block and link projections; nested block parent relationships are retained. | Publication preparation still performs substantial filesystem/parse work while holding the Ledger transaction; concurrency/latency qualification remains partial. |
| Identity and lifecycle | Equal-content copies retain distinct document identities; unchanged tombstoned sources are reprocessed; Markdown reconciliation excludes imported conversions; stable node IDs no longer use unrelated global order; erasure policy survives index rebuild. | Qualified move/rename identity and alias-history transitions remain partial. |
| Scope and source policy | Exact scope-grant read ranges survive catalog persistence and native federation; repository-local Git-ignore semantics, mandatory exclusions, source-read authority, cancellation/deadline and byte/item bounds are shared across scan/query/read. | Runtime revocation/race behavior still needs executed and packaged acceptance evidence. |
| Links and navigation | Forward links, scope-filtered backlinks/link health, parent/previous/next/children/breadcrumb navigation, bounded one-hop strong-seed graph expansion and structural manifests/drift diagnostics are source-wired. | LDG-029/031 remain unqualified until their acceptance cases execute through the managed/product path. |
| Native federation | `ProviderId::Ledger`, daemon owner binding and native provider registration are present; candidates retain exact resolver evidence and Pull remains final admission/fusion authority. | Automatic live candidate delivery remains intentionally gated: the release-specific qualification allowlist is empty and the provider enable flag defaults off. |
| Source reading | Exact registered nodes, imported snapshots, raw/projection/span hashes, continuation cursors, source-bound tickets and current policy checks share the daemon resolver. | Installed host round-trip and revocation-between-retrieval/read acceptance are not yet qualified. |
| CLI/MCP | Operational Ledger CLI is a daemon client; MCP exposes `membrane_ledger` plus enhanced `membrane_source_read`, including `related`, backlinks, manifests and drift. | Product-host capability negotiation still needs installed evidence; tray-off behavior is source-designed but not package-tested in this change. |
| Conversion | Deterministic internal conversion, raw/projection provenance and imported-snapshot resolution remain available internally. Public conversion ingest was withheld from the generic MCP/CLI surface. | Per-format semantic/integrity/installed round-trip qualification is required before a format is advertised. |
| Delivery | Source and test targets compile under the managed check lane; previous FTS engine evidence remains historical. | No release, package, runtime activation, installed CodeRight session or delivered-evidence qualification is claimed. |

The earlier held-out engine result (legacy Recall@5 0.14 versus Ledger FTS 0.68 on 50 queries) remains historical evidence for retaining FTS5. It does not qualify this changed owner/provider/resolver composition.

The source branch therefore closes the principal implementation and wiring defects identified by the audit, while deliberately leaving **qualification and a small set of behavioral contracts partial**. In particular, LDG-004/024 move history, LDG-017/018 publication/concurrency behavior, LDG-022 installed automatic delivery, and LDG-028 per-format qualification must not be promoted from source presence alone.

# 6. Selected target architecture

```text
canonical granted source + source-collection manifest
    -> deterministic conversion, only where qualified
    -> one complete source-positioned GFM parse per changed projection
    -> document / section / typed-block projection
       + identity / aliases / revision and span evidence
       + Ledger-local FTS5
       + source-derived forward/reverse link projections
       + bounded structural-manifest diagnostics
    -> eligible exact-key / lexical / explicit-literal candidate lanes
    -> bounded structural and strong-seed link alternatives
    -> daemon-owned native Ledger source/provider
    -> Pull's existing fusion, admission, budget and publication gates
    -> shared exact resolver with current grant and captured source tuple
    -> optional Push representation with raw recovery
    -> actual host-delivered evidence and omission receipts
```

MCP/CLI/CodeRight use the existing authenticated transport; none owns another index. Engine mode, index readiness and provider-delivery activation remain separate state dimensions.

# 7. Projection and identity model

## 7.1 Parse once; keep internal completeness separate from presentation

The internal projection builder consumes the full bounded source revision, not a paginated outline response. It covers document roots, headings, paragraphs, fenced and indented code, lists/items, blockquotes, tables/rows/cells, HTML blocks, thematic breaks where useful, links/images/reference definitions, footnotes where supported and nested containers.

Page the external outline only after constructing the internal tree. An explicit ingestion limit can produce typed incomplete coverage; it cannot produce a complete publication claim. Avoid arbitrary character chunks as the primary model. Keep genuine parent relationships for nested containers.

For a conversion, the parse input is the verified normalized projection. Raw-source and normalized-source coordinates are different domains; never present a Markdown byte offset as a raw PDF/DOCX offset without a qualified mapping.

## 7.2 Persisted node evidence

The internal record includes, at minimum:

```text
doc_id; source_collection_id; logical_node_id; node_version_id
parent_id; ordinal; node_kind; heading_path; human_anchor_aliases
raw_source_identity; raw_revision; raw_content_hash
projection_hash; projection_version; converter_config_digest, when applicable
source_range; span_hash; searchable_text; source-derived link_targets
parser_version; projection_schema_version; fts_schema_version
tokenizer_id; query_normalizer_version; ledger_generation
coverage; supported_resolution_unit
```

These are internal requirements, not permission to invent unversioned public V2 fields. Map them to existing contracts or approved compatible extensions with schemas/handlers/tests updated together.

## 7.3 Identity, copies, moves and aliases

Separate four concepts: authoritative document identity, logical structural node identity, versioned source-span evidence and human-readable alias. Hashes identify bytes, not uniquely the source or authorizing scope. An ordinal or slug is not a stable identity across edits.

Resolve identities through complete source manifests, source-owner move evidence, stable explicit source IDs where available, structural context and qualified history. Two simultaneous equal-content files remain different documents. Move/copy ambiguity yields a typed unresolved transition rather than choosing the first hash match. Ordinary edits preserve the correct source identity.

Indexing is read-only with respect to source documents. Do not insert HTML comments into code fences or other source text. Do not repoint inbound source links during indexing. Source editing, if separately requested, remains an authorized action outside ordinary indexing.

## 7.4 Exact range resolution

For native Markdown, `span_hash = sha256(source_bytes[source_range])`. Resolve the requested supported unit with the caller's captured source identity/revision/raw hash and span evidence, and return the actual byte range served. A versioned node reference must resolve through the registry owner, not be reinterpreted as a section slug.

Return a larger parent only as an explicit bounded expansion with its own identity and cost. A matched table cell or paragraph must not silently become a document-sized answer. A chunked transport must report page ranges separately from the enclosing source span and guarantee forward progress for nonempty pages.

Identical spans in different locations retain distinct citation context. A content-fingerprint tie alone cannot justify a relocation. Relocation returns verified replacement identity/provenance, or ambiguity; it is not merely a label for a missing anchor.

# 8. Query processing contract

## 8.1 Unicode and identifiers

Retain the original query as request data and derive a versioned normalized query. Evaluate normalization/case handling/combining marks, paths, Markdown punctuation, CJK, mixed scripts and short identifiers. Nonempty text must not silently disappear into a success-with-no-terms path.

Preserve developer identifiers while admitting bounded components, such as `LedgerDb` and `ledger`/`db`, or a path plus its components. Do not explode aliases so broadly that query cost or false positives become unbounded. Actual implementation terminology must match its normalization behavior; a case-folding label is not proof of full Unicode case folding.

## 8.2 Exact-first short queries

LDG-008 remains the exact path/title/anchor/identifier lane ahead of broad lexical retrieval and strong-seed expansion. It is distinct from literal content matching. Safe query construction must own all FTS syntax, control Boolean policy and expose the executed lane/configuration in bounded receipts. Raw user strings must not become SQL or FTS operators.

## 8.3 Explicit literal matching — LDG-030

A caller can explicitly request a literal match. A qualified quote-syntax convention may select that mode, but the system must not reinterpret arbitrary task prose or silently normalize literal bytes. Match case, punctuation and whitespace according to a named explicit mode; the default literal proof is exact byte equality within the named supported source span.

FTS may narrow candidate discovery but cannot establish exactness or complete literal absence. Verify matches against hash-validated bytes, returning exact positions and source evidence. Tokenization can miss punctuation-only or normalization-sensitive literals. Such requests need a bounded source-span scan or another qualified candidate mechanism; otherwise return unsupported/incomplete, not complete no-match. Exhausting a top-k candidate list is also not proof of global absence.

Initially cover supported code blocks. Inline code/configuration tables may be covered only when their supported resolution units and fixtures qualify. Literal matching of converted Markdown is explicitly against the normalized projection, unless a qualified raw-source locator is requested. Do not imply that normalized text appeared byte-for-byte in a binary original.

## 8.4 No-answer and coverage

Evaluate false-positive acceptance and justified abstention separately from positive-query recall. Carry searchable corpus coverage, candidate-discovery limits and stale/unsupported omissions safely. Do not leak denied file names, counts or links while explaining an incomplete result. Pull decides whether the overall task has sufficient evidence.

# 9. Ledger-local FTS

Retain the Ledger-owned FTS5 schema and weighted path/title/heading/body/identifier fields. SQLite's `bm25()` accepts query-time column weights; hardcoded values in current code are a configurable policy choice, not a reason to replace the engine. A generic second-engine abstraction is not required here.

Tokenizer alternatives, field weights, Boolean query strategy, field normalization and breadcrumb augmentation are tuned only on development data. Freeze a candidate before held-out evaluation. Query-weight profiles, if introduced, must be named, versioned, bounded and host-policy controlled, not arbitrary authority granted by document text.

Active FTS qualification proves that the shipped recall path executes `MATCH`/ranking. Index existence, unchanged output in a test or a switch set in storage is not that proof. Track source-local rank and lane evidence; Pull owns heterogeneous fusion and does not compare raw BM25 values directly with unrelated provider scores.

An alternative engine requires a measured unmet requirement and explicit cost/compatibility review. DisMax or another ranking operator cannot enforce eligibility, freshness or safety, regardless of its numeric aggregation rule.

# 10. Structural, link and reverse-reference navigation

## 10.1 Hierarchy and candidate budgets

Expose bounded parent, child, same-parent sibling and ancestry queries. Distinguish result-count budgets from candidate-pool and expansion-work budgets. Graph alternatives may compete when base top-k is full, but must not create unbounded expansion or bypass Pull admission. Preserve expansion provenance and typed abstention through the final service response rather than discarding the trace.

## 10.2 One link resolver

Generator, validator, indexing, navigation and resolution share one semantic resolver for inline/reference/autolink/image/relative/fragment/Unicode/broken targets. Edges bind the exact source link span and current target identity/revision/span where resolved. A link never grants access to its target. Broken, denied and ambiguous states remain distinct without exposing inaccessible target metadata.

## 10.3 Strong-seed expansion

Use qualified strong seeds only. Enforce maximum hops, nodes, edges, source bytes, elapsed work and inherited cancellation; detect cycles and explicitly abstain on weak/unusable evidence. No generic semantic relation traversal is introduced.

## 10.4 Inbound references and link health — LDG-029

Add a bounded reverse-navigation operation over the existing source-derived edges and target indexes. Return which eligible document/section spans cite a target, with generation and resolution provenance. Reuse the existing storage/edge model; do not create a second graph authority.

Reference counts, orphan candidates, broken links and cycles are diagnostic views. Counts apply only to the caller-visible declared source set and graph coverage. A document with zero visible inbound edges is not necessarily globally orphaned. Return unknown/partial when coverage or freshness cannot justify a complete claim. Materialized reverse indexes are allowed if deterministic, rebuildable and generation-bound; derived does not mean everything must be recomputed at read time.

Link popularity is not independent corroboration and does not increase authority. Any influence on relevance ranking needs a separate ablation with no-authority-gain tests. Return bounded query results; do not place the full graph in every receipt.

# 11. Contextualization and result-granularity experiments

Retain title-chain contextualization as an experiment, not an assumed improvement. Compare no prefix, deterministic breadcrumbs and alternative field weighting using retrieval quality, index cost, latency and delivered-token cost.

Group overlapping document/section/block results so the same source text is not counted repeatedly as useful evidence. A bounded parent merge is permitted as a candidate representation when it improves complete evidence per delivered token, preserves source order and retains exact child/source provenance. It is not automatic whole-document expansion.

Future-question aliases remain non-authoritative, source-evidence-bound, separately weighted and shadow-qualified under LDG-027. They invalidate on revision/span drift. No LLM-authored identity, authority or ungrounded document truth is permitted. Structural drift and backlink counts are diagnostic by default, not automatic freshness/quality proxies.

# 12. Evaluation architecture

Use authoring/train, development/tuning and held-out splits. Freeze tokenizer, weights, normalization, exact/literal routing, expansion limits and representation experiments before held-out promotion. Do not turn repeated runs on the old held-out set into a tuning loop.

Use at least the real Membrane documentation and a second real corpus with a different authoring style. Include exact-document/heading, paraphrase, no-answer, path/identifier, CJK/mixed-script, table/list/code, link/reference, stale/moved/duplicate and multi-section synthesis queries. Add literal punctuation/case/whitespace fixtures, scoped backlink queries and structural-baseline expiry cases.

Predeclare practical effect size, corpus sizing or paired-bootstrap intervals. Fifty queries do not establish arbitrary small improvements. Mechanics fixtures diagnose failures but do not replace real-corpus promotion.

Measure Recall@k, MRR and suitable nDCG, exact source/span resolution, stale/relocation/ambiguity correctness, false-positive/no-answer behavior, corpus coverage, delivered unique evidence per token, duplicate-span overhead, p50/p95 cold/warm latency, index/WAL/payload bytes, memory delta and production lane execution counts. For each active operation record the applicable release/configuration and actual host-delivery boundary.

# 13. Runtime, conversion and diagnostic contracts

## 13.1 Operation boundaries

| Operation | Required input | Outcome |
|---|---|---|
| Sync | Authoritative source collection, manifest coverage, grant, source bytes/revisions, effective policy and bounds | Complete atomic generation or typed partial/failure; no cross-owner deletion |
| Recall | Authenticated repository/worktree/source set, query intent, publication, lane/candidate limits, deadline/cancellation | Source-bound candidates, coverage/omissions and lane provenance |
| Resolve | Grant plus captured source/node/version/hash tuple and continuation | Exact supported bytes or typed refusal/relocation/ambiguity |
| Expansion | Eligible strong seeds plus all caps | Bounded structural/link alternatives with provenance |
| Backlinks/health | Named source/section, visible corpus, publication and limits | Coverage-qualified inbound refs and diagnostic counts |
| Drift | Two named source-bound manifests and limits | Deterministic structural delta or explicit missing/expired/incompatible baseline |
| Activation | Mode, trusted receipt and compatible release/configuration tuple | Explicit active mode or typed refusal |
| Erasure | Authorized source identity and projection inventory | Transactional logical erasure plus readback/retention accounting |
| Rebuild | Canonical eligible source manifest and versions | Equivalent current projection; historical coverage explicitly bounded |

Operational requests flow through one daemon-owned service. Membrane owns deadline creation and propagation; Ledger honors the inherited deadline rather than constructing an independent scheduling authority. Existing public V1 shapes remain unless a real consumer requires a versioned/compatible extension.

## 13.2 Per-format conversion qualification — LDG-028

The backend boundary declares actual supported formats, input/resource limits, deterministic configuration, supported structural outputs, raw-source resolution and typed error/loss behavior. Prefer a simple declarative conversion path where available; a paginated/recognition path is a separate complexity tier, not a dependency of basic document retrieval.

The inspected code's text/HTML/JSON mechanisms do not establish support for DOCX, PDF, CSV, EPUB, spreadsheets or presentations. A format may be advertised only after its installed conversion → registration → search → exact-resolution path qualifies. No format count is copied from a donor into Membrane product claims.

Each supported format needs frozen input/expected-output fixtures and semantic invariants for applicable headings, ordering, tables, code, links and Unicode; repeated conversion; dual raw/normalized hash checks; converter/configuration drift; corrupt/unsupported input; output growth and work bounds; loss propagation; and source-ref round trips. Character coverage alone is insufficient. Archive/container limits and denied remote-resource fetches must be addressed by applicable formats rather than hidden inside a generic success result.

Verify the preserved raw input and normalized Markdown independently. Distinguish an immutable snapshot from a live external source. The latter needs current owner freshness; the former keeps snapshot identity and retention. Do not overwrite a caller's expected hash with the newest database hash to make a stale reference resolve.

Media ingestion, page rendering, stochastic structure generation, OCR/VLM pipelines and multimedia embeddings are not added by this revision. Any expansion beyond the existing exclusions needs a separate architecture/scope decision. Borrowing a Docling-style interface does not make Docling a mandatory runtime dependency.

## 13.3 Structural drift diagnostics — LDG-031

Compare two explicitly named source-bound structural manifests. Include document identity, raw/projection revisions and hashes, configuration identity, counts/IDs of added and removed units, qualified moves and ambiguous mappings. Define units and denominators before reporting a percentage; separate parser/converter changes from source edits.

Retain only the bounded compact manifests/history needed by this operation under explicit privacy/retention policy. No indefinite full-document version store is implied. A missing, expired, erased, unreadable or incompatible baseline is not an empty prior document and cannot justify mass-removal claims. Retained diagnostic metadata is erased with the owning source where required.

The result is read-only evidence. It does not prefer stable documents, rewrite knowledge, issue semantic contradictions or automatically invalidate the truth of a Cortex record.

## 13.4 Coverage, status and operational observability

Status distinguishes daemon liveness, enrollment, grant/source-set validity, last successful sync, publication and source clocks, pending updates, corpus coverage, retrieval mode, provider delivery mode, qualification compatibility and resolver capability. Inspecting inactive status does not create a store.

Content-free lifecycle events allow operators to diagnose update, conversion, query, resolution, erasure, cancellation and rollback failures. Raw document/query content is not logged by default. Selected candidates, resolver-backed references, successful reads and host-rendered evidence are different observations and must not be conflated in delivery accounting.

# 14. Session document projection

Session Markdown is a derived human-readable projection of underlying typed events/tasks/artifacts/decisions. It is not the document subsystem itself, durable learned memory or a replacement for structured source data.

A projection carries source session identity, source cursor/digest, derivation version, content hash, omissions and invalidation parent. It remains non-recallable until a real consumer, authority contract, privacy/retention, lifecycle and replay value qualify. Keep session records distinct from document registrations. Adapt consumes authoritative structured observations instead of derived Markdown when both are available.

Retain LDG-020/021/025 distinctions. Do not close a missing production consumer by simply making all generated session documents searchable.

# 15. Cross-subsystem effects

## 15.1 Pull and host delivery

Ledger materializes source-local candidates through the native owner handle. Pull owns provider admission/invocation, cross-provider fusion, eligibility fences, sufficiency, the final budget and publication. Those responsibilities remain separate in LDG-022 and PUL-015.

A normal context request must prove the native Ledger source actually ran. Every selected resolver-backed result must have a usable operation in the recipient host's negotiated capability set, a valid source tuple and recoverable continuation. A generic discovery payload or knowledge of an unadvertised tool name is not sufficient. Prefer the existing generic source reader; add bounded navigation operations only where a named consumer requires them.

## 15.2 Cortex and conditional source-change work

Cortex may retain Ledger references without importing the document corpus. Current source resolution/re-anchoring goes through Ledger. Both LDG-023 change notifications and CTX-033 document-derived semantic revalidation remain exploratory/HOLD in this revision.

If separately promoted, an owner-local ordered journal carries reference-only notices, publication identity and idempotency identity. Define durable cursor checkpoints, floor/head, bounded retention, restart/replay, duplicate handling and typed `rescan_required` on gaps. A reset-to-zero shortcut is not recovery once history has been compacted. Notices do not remove the need to resolve current bounded evidence under the current grant.

A changed or unavailable reference can trigger Cortex-owned reference revalidation; it does not prove a durable statement false. Keep reference health distinct from semantic truth, contradiction, supersession and retirement. Ledger never directly modifies Cortex records or supplies an authoritative truth class. No new Cortex atom is allocated by this Ledger revision; any future promotion must update Cortex's owner canon explicitly.

## 15.3 Blueprint

Consume typed source/revision and repository-truth services without direct database access. Respect Blueprint's document-domain convergence dependency and do not claim Ledger repair alone solves it. Any automatically pending domain needs a bounded automatic completion or a visible terminal failure.

## 15.4 Push

After Pull selection, Push may reduce resolved blocks while preserving code fences, tables, links, protected spans, source order and raw recovery. Keep the exact source and representation provenance distinct. Compression is not permission to alter literal-match evidence or hide material omissions.

## 15.5 Adapt

Adapt may consume structured retrieval/use/failure observations and propose alias, normalization, projection or ranking changes. Proposals pass Ledger's development/held-out and promotion gates; Adapt does not directly mutate active policies or projections.

Useful evidence includes repeated irrelevant hits, sections later found manually, failed relocation, eligible identifier/CJK/literal misses, selected evidence never used, user corrections and evaluator outcomes. Distinguish advisory feedback from verified host/outcome evidence.

## 15.6 CodeRight and other harnesses

Hosts emit the existing task/session identity, genuine context-capacity observations, context receipt, used candidate/source IDs, resolver outcomes, manual recovery searches and trusted task outcome where available. Do not invent a large capacity value merely to bypass the production request contract.

A host cannot claim Ledger access because it can read a filesystem file or run a local CLI manually. The relevant proof is discovery → native retrieval → Pull decision → exact resolution/continuation → delivered evidence through the supported installed binding.

# 16. Production-path evidence invariant

A capability is not landed until production reachability and frozen applicable acceptance prove it. ASTs must be emitted by shipped sync, active search must execute the real lane, links/backlinks must be used by their supported consumers, source reads must work through advertised transports, and exact installed builds must exercise the same path where packaging matters.

Keep four qualification dimensions separate: retrieval-engine quality, provider admission/materialization, installed transport/resolver compatibility and actual host delivery. A trusted receipt's validity does not prove applicability to a new release/configuration. Bind release/build, parser/projection/tokenizer/normalizer/converter configuration, source/provider/resolver versions and corpus/query-set identities.

Documentation updates cannot promote implementation, focused verification, qualification, delivery or competitive closure. Reopen a widened acceptance contract while preserving earlier evidence as historical. Never fabricate a run ID, verifier, receipt hash, successful test or installed state.

# 17. Implementation sequence

| Package | Work | Exit boundary |
|---|---|---|
| P0 — Reproduce and reconcile | Capture source-derived failures, update this architecture/atomic intake and keep earlier evidence intact | Minimal fixtures and explicit baseline; no successful runtime claim |
| P1 — Source integrity | Grant/source manifests, copy/move identity, Git-ignore semantics, source-owner reconciliation, resurrection, erasure | Multi-root, equal-content, mixed-format, revoked-source and rebuild correctness |
| P2 — Complete projection/resolution | One complete parse, full typed ancestry, exact node/source tuples, dual conversion hashes and transport continuation | Query-to-exact-read round trip for every advertised source kind |
| P3 — One daemon owner | Replace operational local Ledger CLI paths with existing authenticated service operations | Tray-off/no-index parity and common CLI/MCP owner |
| P4 — Native provider/host closure | Bind document source, native provider and usable host resolver through Pull | Installed normal-context request yields candidate, decision and exact delivered bytes |
| P5 — Measured query/navigation | Exact-first, overlap handling, bounded structural/graph alternatives, LDG-029 and LDG-030 | Frozen quality, literal integrity, scope and boundedness evidence |
| P6 — Diagnostics/update bounds | Coalesced source updates, short publication windows, truthful coverage/status, LDG-031 | Drift-baseline safety, crash/cancellation and bounded cross-root interference |
| P7 — Qualification/release | Applicable receipts, package/host tests, rollback and generated state reconciliation | Exact release evidence for each claimed capability |

P5/P6 are selected scope but are not pretexts to postpone the integrity and reachability repairs. A core-path milestone is not “all Ledger complete” while any committed atom remains open. LDG-023/CTX-033 and a second search engine are not dependencies of P1–P7.

# 18. Test architecture

| Layer | Required cases |
|---|---|
| Parser/projection | 255/256/257/1,000 headings; headingless text; Unicode/duplicate headings; nested lists/quotes/tables; code fences/indented code; links; exact ranges; parse-count instrumentation |
| Identity | Two roots/same relative path; equal-content copies; divergent edits; true moves; ambiguous copies/merges; duplicate spans; source bytes unchanged by indexing |
| Lifecycle | Add/delete/sync/identical-byte reappearance; converted source plus Markdown scan; unavailable scan; ignored/excluded/revoked source; no resurrection after erase |
| Query/FTS | Unicode/CJK/identifier/short queries; safe operators; deterministic ranking; actual active `MATCH`; exact-first and no-answer ablations |
| Literal | Case/punctuation/spacing/mixed-script distinctions; punctuation-only/no-token fallback; partial candidate discovery; normalized-versus-raw coordinate labeling |
| Resolver | Exact registry-supported node; captured revision/hash/span; all continuation pages; changed source between pages; explicit parent expansion; current grant on every read |
| Conversion | Each advertised format's semantic preservation and typed losses; dual hashes; config drift; immutable snapshot/live-source differences; resource/error fixtures; installed round trip |
| Links/backlinks | Forward/reverse parity; fragments; broken/ambiguous/Unicode refs; denied targets; cycle/weak seed; coverage-qualified orphans; stale/deleted link source; full top-k expansion pool |
| Drift | Named manifest delta; qualified versus ambiguous move; baseline expiry/erasure/unavailability; converter-only changes; defined denominators; deterministic output and retention |
| Storage/concurrency | Crash before publish; corrupt/unknown versions; coherent request tuple or typed retry; bounded generation updates; cancellation; full/incremental equivalence |
| Service/host | Native provider execution; advertised source reader and consumable cursor; tray off; no status-created DB; same handler across transports; actual delivered-evidence receipt |
| Qualification | Config/release mismatch; separate engine/provider/host promotion; real rollback; no old proof relabeled as current |
| Cross-owner safety | No Cortex/Blueprint direct DB reads; no Ledger source mutation; no reference-count authority gain; no changed-source fact retirement; session recall remains gated |

All cases are required tests, not results claimed by this revision. Use repository-managed CI for Rust/package execution and supported-host evidence for installed behavior.

# 19. Acceptance gates

## Hard gates

The relevant active capability must satisfy source identity, current grant, exact range/hash, complete-or-explicitly-partial coverage, supported-format qualification, coherent generation, bounded work, correct erasure, usable host operations and genuine production-path execution. The former source-identity, 256-section, lifecycle and transport defects have branch source repairs and must not regress; their source presence still does not justify a claim of complete automatic document delivery without executed release-specific evidence.

No candidate score, backlink count, source stability, model assertion or valid-but-inapplicable receipt can compensate for failure of a hard gate. Session projection stays gated; cross-owner DB reads, source rewriting and unapproved semantic/media scope remain prohibited.

## Measured gates

Freeze candidates before held-out; use predeclared paired quality/uncertainty, latency/storage/memory and delivered-evidence efficiency. Prove useful graph alternatives, literal precision and justified negative behavior. Backlinks/drift need correctness and operational value, not a presumed ranking win. Qualification must bind the actual compatible package and rollback path.

# 20. Rejected designs

Reject a second Guide identity, `ledger::ledger`, dual-source authoritative Markdown caches, a second planner, direct Cortex/Blueprint storage use, default vectors, unbounded graphs or query rewriting, arbitrary structure-blind chunks, generated summaries as truth, source-mutating ID insertion, first-hash-match move detection, whole-corpus erasure inferred from one scanner, presentation-limit index truncation, false complete-empty after observation failure and local operational CLI fallback.

Also reject immediate Tantivy/`LexicalEngine` adoption without measured need, DisMax as policy enforcement, FTS phrases as byte-exact evidence, link counts as corroboration, low edit rate as correctness, raw source content in change notices, cursor-zero recovery without retained history, automatic Cortex truth retirement on source change and exploratory scope promotion hidden in an implementation plan.

# 21. Research basis, source register and disposition

This revision uses the supplied commit-pinned audit, companion `membrane-ledger.md` and the subsequent acceptance/correction review. The companion's donor labels are not independently adopted as comprehensive code-audit or performance claims. Source-specific ideas are adapted within Ledger's existing ownership boundaries.

| Reference | Retained design contribution | Limit |
|---|---|---|
| Pagefind | Heading-level result grouping and matched-location affordances | Not another authoritative search store |
| Marksman / markymark | Consistent navigation/reference semantics and shared engine-to-agent interface | Not another required resident or unreviewed dependency/license adoption |
| ripgrep `ignore` | Mature matching/precedence semantics for source enumeration | Ignore rules never grant access |
| LlamaIndex parent merging / PageIndex | Bounded structural navigation/representation experiments | No mandatory vector or LLM tree-search dependency |
| Docling backend contract | Declared formats and declarative/paginated backend separation | No wholesale converter replacement or transferred format count |
| Siyuan companion reference | Block-addressability and explicit relocation as design ideas | No imported source mutation or automatic link rewriting |
| SQLite FTS5 documentation | Column weights and tokenized phrase behavior | Ranking configuration does not prove literal equality or delivery |
| Tantivy companion reference | Possible future measured engine comparison | Not selected; numeric ranking is not hard eligibility |

The older research disposition remains: GFM/CommonMark/source-position structure, build-time projections and deterministic navigation are directly relevant; title chains and contextual retrieval are ablations. Heterogeneous fusion belongs in Pull; query-aware reduction and raw recovery belong in Push. No research name is itself a production gate.

Primary baseline and verification references:

- [Atomic Ledger baseline](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/canon/ledger.md).
- [Prior architecture, including preserved ownership and rollout](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/architecture/subsystems/ledger.md).
- [Indexing, FTS, query normalization and block nodes](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/ledger/index.rs).
- [Source registration, lifecycle, recall and converted-source loader](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/ledger/doc_spine.rs).
- [Outline, span identity and continuation](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/ledger/outline.rs).
- [Storage and link-target indexes](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/ledger/db.rs).
- [Native source composition](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/pull/federation_sources.rs) and [provider registry](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/pull/native_federation.rs).
- [MCP executor](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/mcp_executor.rs) and [tools/negotiation](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-mcp/src/tools.rs).
- [Earlier recorded Ledger evaluation](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/evidence/qualification/ledger-metrics.json).
- [Cortex exploratory reference-revalidation contract](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/canon/cortex.md).
- [Atomic-canon checker and register/receipt requirements](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/scripts/ci/check-atomic-canons.mjs).
- [SQLite FTS5](https://www.sqlite.org/fts5.html), [Docling backend interface](https://github.com/docling-project/docling/blob/main/docling/backend/abstract_backend.py), and [Tantivy DisjunctionMaxQuery API](https://docs.rs/tantivy/latest/tantivy/query/struct.DisjunctionMaxQuery.html).

Source-derived current-state claims are confined to the reviewed baseline. Normative additions are the selected design for this authored revision, not facts asserted about currently installed software.

# 22. Final canonical statement

> Ledger is Membrane's granted document registry, structural indexing, navigation, source-local retrieval and exact source-resolution subsystem. It owns rebuildable source-bound AST/FTS/link/conversion projections and bounded reference/drift diagnostics, while authoritative bytes remain with their source owner. It is operated by the tray-owned daemon and consumed through usable host contracts. Blueprint owns repository truth, Cortex durable knowledge, Adapt proposals, Pull final context admission/fusion, Push faithful reduction, and CodeRight agent execution. A capability is complete only when its actual supported production path and applicable acceptance evidence prove it—not when its table, helper or architecture diagram exists.
