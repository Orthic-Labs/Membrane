# Ledger final revision: scope and source reconciliation

Date: 2026-09-05. Baseline: `75c257ad711d19ffce69258d132a45dbffa9b4ac`. This is an authored design-intake and static-source reconciliation, **not** a runtime, focused-test, qualification, release, or independent-verifier receipt.

## Authority and scope

The user requested: “please give me a final md please. Does this affect the canon and add atoms too? if yes, please give me the revised canon file too”. This authorizes preparing this final plan and canon revision. It does not claim that any file has been applied to the repository, nor authorize release, runtime activation, source-document rewriting or promotion of existing exploratory work.

The three new capabilities are recorded as COMMITTED **scope in the proposed canon for adoption**, not delivered implementation. They are all MISSING / PENDING / PENDING / LOCAL. LDG-023 and CTX-033 remain EXPLORATORY/HOLD; no other subsystem's scope is promoted.

## Inputs

| Input artifact | SHA-256 |
|---|---|
| Membrane_Ledger_Audit_and_Improvement_Plan_2026-09-05.md | 41c20e0d6d8197f9c0a0bbcc64c22b443516a70342fd78c279f2a674b47a9eaf |
| membrane-ledger.md | 7803dc03b44755a510aeef49a5560671c531a0c9c2452c69bb187d5ea3eda640 |

The repository head was re-read through the connected GitHub integration and remained `75c257ad711d19ffce69258d132a45dbffa9b4ac`. Source findings are inherited from the commit-pinned audit plus the connector reads cited below; no Rust, installed application or live CodeRight execution was performed in this consolidation.

## New capability register

| Introduced ID | Origin | Observable behavior | Authority/evidence |
|---|---|---|---|
| LDG-029 | 2026-09-05 Ledger final consolidation; source proposal in membrane-ledger.md | Enumerate scope-filtered, generation-bound inbound document/section references and bounded link-health diagnostics from source-derived edges; qualify orphan/refcount results by visible corpus coverage and never treat link count as authority. | User-requested final canon revision, 2026-09-05; docs/provenance/foundation/2026-09-05-ledger-final/reconciliation.md |
| LDG-030 | 2026-09-05 Ledger final consolidation; source proposal in membrane-ledger.md | Resolve explicitly requested literal content matches only after exact byte verification against eligible hash-validated source spans, preserving punctuation/case/whitespace and reporting incomplete search rather than false complete no-match. | User-requested final canon revision, 2026-09-05; docs/provenance/foundation/2026-09-05-ledger-final/reconciliation.md |
| LDG-031 | 2026-09-05 Ledger final consolidation; source proposal in membrane-ledger.md | Report deterministic structural deltas between named source-bound document manifests, distinguishing additions/removals/qualified moves/ambiguity from unavailable baselines; keep drift metadata diagnostic, bounded & erasable. | User-requested final canon revision, 2026-09-05; docs/provenance/foundation/2026-09-05-ledger-final/reconciliation.md |

The exact three rows above must also be inserted into the existing `## New capability register` in `docs/provenance/migrations/2026-08-30-atomic-canons/preservation-map.md`. Do not alter the 728 preserved legacy/specification rows or the split register. A separate support file supplies the additions.

## Amended existing contracts and historical state

Seventeen existing observable contracts are strengthened in place. IDs, owner and parent relationships are retained. Their new full contracts remain PARTIAL; prior focused passes become STALE where the acceptance boundary widened, and the revised definitions have LOCAL delivery. Historical assertions below remain historical—not evidence for the expanded contract. This does not assert that already implemented FTS, Unicode, link or projection machinery vanished.

| Atom | Baseline observable behavior | Baseline implementation/verification/qualification/delivery | Baseline competitive state | Historical evidence |
|---|---|---|---|---|
| LDG-001 | Register eligible Markdown sources with canonical identity, revision, content hash, & grant. | PARTIAL/PENDING/PENDING/LOCAL | CURRENT_INCOMPLETE | PENDING |
| LDG-002 | Parse each revision once into source-positioned GFM structure spanning headings, prose, code, lists, quotes, tables, HTML, links, & nested blocks. | PARTIAL/PENDING/PENDING/LOCAL | CURRENT_INCOMPLETE | PENDING |
| LDG-003 | Persist ordered document/section/block hierarchy with spans, search text, links, revision, & generation. | PARTIAL/PENDING/PENDING/LOCAL | CURRENT_INCOMPLETE | PENDING |
| LDG-004 | Derive stable section identity from body span hash + structural context while keeping slug/ordinal as aliases. | PARTIAL/PENDING/STALE/PUSHED | CURRENT_INCOMPLETE | Acceptance: LDG-004; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-005 | Read exact current section only when document hash, anchor, & span verify; paginate by continuation cursor. | DELIVERED/FOCUSED_PASS/PENDING/PUSHED | CURRENT_BEST | Acceptance: LDG-005; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-006 | Return typed current/relocated/stale/missing/denied/unavailable/ineligible/unsupported outcomes; observation failure never becomes negative evidence. | PARTIAL/PENDING/STALE/PUSHED | CURRENT_INCOMPLETE | Acceptance: LDG-006; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-008 | Route short queries through exact path/title/anchor/identifier before FTS & bounded structural expansion. | PARTIAL/PENDING/STALE/LOCAL | CURRENT_INCOMPLETE | PENDING |
| LDG-012 | Bind FTS activation to trusted qualification receipts & fail loudly on evidence drift. | DELIVERED/FOCUSED_PASS/PENDING/PUSHED | CURRENT_BEST | Acceptance: LDG-012; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-014 | Retrieve parent, child, sibling, & heading ancestry from persisted hierarchy. | PARTIAL/PENDING/PENDING/LOCAL | CURRENT_INCOMPLETE | PENDING |
| LDG-016 | Expand only strong seeds under hop/node/edge caps, cycle detection, provenance, & typed abstention. | DELIVERED/PENDING/PENDING/PUSHED | DONOR_BETTER | Acceptance: LDG-016; Revision: 8c892cf02fa62d1c1211f06755b5478acfa5a0d1; Receipt: docs/provenance/foundation/2026-08-31-donor-better-implementation.md@1620ca701c8cfd99d01644ed52ad4709b293b91f; Freshness: 2026-08-31 |
| LDG-017 | Publish one transactional generation so readers never observe mixed nodes, links, FTS, or artifacts. | DELIVERED/FOCUSED_PASS/PENDING/PUSHED | CURRENT_BEST | Acceptance: LDG-017; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-018 | Incrementally sync changes, tombstone removals, & rebuild an equivalent projection. | DELIVERED/FOCUSED_PASS/PENDING/PUSHED | CURRENT_BEST | Acceptance: LDG-018; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-019 | Erase every Ledger-owned projection for granted source identity. | PARTIAL/PENDING/PENDING/LOCAL | CURRENT_INCOMPLETE | PENDING |
| LDG-022 | Materialize source-bound Ledger section candidates with generation, provenance & freshness for an authorized Pull provider. | PARTIAL/PENDING/PENDING/LOCAL | CURRENT_INCOMPLETE | PENDING |
| LDG-024 | Persist document alias history across resolution changes. | PARTIAL/PENDING/PENDING/PUSHED | CURRENT_INCOMPLETE | Acceptance: LDG-024; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-026 | Admit only confined, canonical worktree document references before resolution. | DELIVERED/PENDING/PENDING/PUSHED | UNRESOLVED | Acceptance: LDG-026; Revision: f42b6c96611cd98fa06eb21360e2b1389c67527a; Receipt: docs/provenance/migrations/2026-08-30-atomic-canons/source-consumer-reconciliation.md@efb0eb1bc08b3f0e11e74a2a44fb3db17d4a9e08; Freshness: 2026-08-30 |
| LDG-028 | Normalize granted non-Markdown document formats deterministically into hash-bound Markdown input while retaining raw-source resolution, converter/version/config provenance, & typed loss/omission accounting; exclude media ingestion. | DELIVERED/PENDING/PENDING/PUSHED | DONOR_BETTER | Acceptance: LDG-028; Revision: 8c892cf02fa62d1c1211f06755b5478acfa5a0d1; Receipt: docs/provenance/foundation/2026-08-31-donor-better-implementation.md@1620ca701c8cfd99d01644ed52ad4709b293b91f; Freshness: 2026-08-31 |

Unchanged capability rows LDG-007, LDG-009, LDG-010, LDG-011, LDG-013, LDG-015, LDG-020, LDG-021, LDG-023, LDG-025 and LDG-027 retain baseline state/evidence. Their unchanged historical comparison pins remain intact. Existing architecture obligations, cross-subsystem release gates and new acceptance suites still apply; preserved FOCUSED_PASS is not a new test claim.

## Disposition of companion proposals

| Proposal | Final disposition | Owner |
|---|---|---|
| Per-format backend contract and qualification | Accepted as LDG-028 strengthening; no mandatory Docling replacement and no new supported-format promise | Ledger conversion and shared resolver |
| Backlinks, reference counts, graph health | LDG-029; source-derived, bounded and coverage-qualified; existing outgoing graph is retained | Ledger navigation |
| Literal matching | LDG-030; verify exact bytes and report search incompleteness; FTS escaping is not literal proof | Ledger query and exact resolver |
| Block identity and ref transfer | Strengthen LDG-004/005/024; no source mutation and no identity inferred solely from equal content | Ledger identity/resolution |
| Structural drift | LDG-031; diagnostic delta between retained source-bound manifests, not preference for old text | Ledger diagnostics |
| Durable change feed | LDG-023 remains exploratory; reference-only notices do not replace freshness checks | Ledger, conditional on separate promotion |
| Cortex stale-reference response | Future owner revalidation under CTX-033; a changed citation does not falsify or retire durable facts | Cortex, exploratory |
| Tantivy and LexicalEngine abstraction | Deferred; FTS5 already accepts query-time column weights; no measured requirement justifies a second engine | Future measured architecture decision |
| DisMax as an authorization or non-compensatory policy mechanism | Rejected; ranking does not enforce hard eligibility | Pull/Membrane remains authority |
| Injected in-document block IDs | Rejected as indexing behavior; author-provided IDs may be read as aliases | Source owner retains editing authority |

## Source pointers

- [Atomic Ledger baseline](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/canon/ledger.md)
- [Architecture baseline](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/architecture/subsystems/ledger.md)
- [Canon checker](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/scripts/ci/check-atomic-canons.mjs)
- [Cortex CTX-033](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/docs/canon/cortex.md)
- [Ledger source/identity/lifecycle](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/ledger/doc_spine.rs)
- [FTS and block nodes](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/ledger/index.rs)
- [Storage and reverse-target index](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/ledger/db.rs)
- [Native owner binding](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/pull/federation_sources.rs)
- [Native registry](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/pull/native_federation.rs)
- [MCP resolver](https://github.com/Orthic-Labs/Membrane/blob/75c257ad711d19ffce69258d132a45dbffa9b4ac/engine/crates/membrane-runtime/src/mcp_executor.rs)

For the external correction, see [SQLite FTS5 BM25 and phrase semantics](https://www.sqlite.org/fts5.html) and [Docling backend contracts](https://github.com/docling-project/docling/blob/main/docling/backend/abstract_backend.py). Donor references are design evidence, not Membrane qualification or license approval.
