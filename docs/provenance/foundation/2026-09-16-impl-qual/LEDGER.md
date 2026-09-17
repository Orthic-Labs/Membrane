# LEDGER lane report — impl qualification

Lane scope: `engine/crates/membrane-runtime/src/ledger/**` only. No file
outside that directory was edited. The portable skill-document adapter
(LDG-032) was implemented inside the owned boundary; the two wiring seams it
needs outside `ledger/**` are recorded under PATCH REQUESTS, not patched.
No git mutation, canon-register edit, install, or direct Cargo was performed
— compile evidence comes from the repository's documented local
unsigned-installer lane (`pnpm --dir apps/membrane-hub run
release:validate:win`, which invokes `rightkit cargo check`).

## ATOM TABLE

Statuses: IMPLEMENTED+VERIFIABLE = mechanism confirmed by inspection and a
native control is wired into `qualification ledger <id>`
(`qualification_core::run` covers LDG-001..015 rows;
`qualification_lifecycle::run` covers LDG-017..032); GAP-REPAIRED = a
lane-owned gap was found and fixed in-place; PENDING-WIRE = mechanism exists
but production wiring lives outside the owned path (see PATCH REQUESTS);
EXCLUDED-SAFEGUARDED = canon-excluded atom whose preserved safeguards were
verified or hardened. No atom is claimed RELEASED: the installed-consumer
boundary was not executed (see BLOCKERS).

| Atom | Status | Evidence |
|---|---|---|
| LDG-001 owner-scoped source registration | IMPLEMENTED+VERIFIABLE | `doc_spine.rs`/`policy.rs`/`reconcile.rs`/`service.rs`; enrolled-root binding, revision/raw-hash manifests, durable exclusions. Native control `run_001` (`qualification_core.rs`); installed row `LDG_001` (`ldg-windows.mjs:330`). |
| LDG-002 single-parse GFM projection | IMPLEMENTED+VERIFIABLE | `index.rs`/`outline.rs`; one Comrak parse feeds complete internal projection, paged external outline. Control `run_002`; installed fixture arm (`ldg-windows.mjs:245`). |
| LDG-003 ordered ancestry + span hashes | IMPLEMENTED+VERIFIABLE | `index.rs`/`db.rs`; section/block nodes with `parent_id`, source ranges, `span_hash`. Control `run_003`; nested-ancestry probe (`ldg-windows.mjs:356`). |
| LDG-004 document/node/fingerprint/alias identity | IMPLEMENTED+VERIFIABLE | `index.rs`/`outline.rs`/`doc_spine.rs`; `ledger.doc:` identity, `stable_node_id`, projection fingerprint. Control `run_004`; identity probe (`ldg-windows.mjs:361`). |
| LDG-005 exact ticketed resolution | IMPLEMENTED+VERIFIABLE | `resolve.rs` node/section reads bind expected revision/content/span hash + generation with continuation cursors (`resolve.rs:219-301`). Control `run_005`; installed read arm (`ldg-windows.mjs:343`). Installed host/revocation acceptance pending. |
| LDG-006 typed outcome vocabulary | IMPLEMENTED+VERIFIABLE | `resolve.rs`/`outline.rs`/`service.rs` typed errors (`Stale`/`Relocated`/`Missing`/`Denied`/`Unavailable`/`BudgetExhausted`/`InvalidCursor`). Control `run_006`; typed-refusal probe (`ldg-windows.mjs:365`). |
| LDG-007 query normalization | IMPLEMENTED+VERIFIABLE | `index.rs` normalization (`index.rs:512-590`) — Unicode/case/punctuation/CJK without erasing nonempty queries. Control `run_007`; recall arm (`ldg-windows.mjs:350`). |
| LDG-008 exact-first routing | IMPLEMENTED+VERIFIABLE | `query.rs` `exact_nodes` (`:203-223`) precedes FTS/graph lanes with lane receipts. Control `run_008`. |
| LDG-009 FTS term escaping | IMPLEMENTED+VERIFIABLE | `index.rs` single safe FTS builder (`:445-509`). Control `run_009`; injection probe (`ldg-windows.mjs:350-351`). |
| LDG-010 separate FTS5 projection | IMPLEMENTED+VERIFIABLE | `db.rs:50-95`, `index.rs:153-375`; `ledger_node_fts` weighted path/title/heading/body/aliases. Control `run_010`; mode probe (`ldg-windows.mjs:371`). |
| LDG-011 deterministic BM25 sections | IMPLEMENTED+VERIFIABLE | `query.rs::fts_nodes` (`:180-201`) generation/revision-bound BM25 over scoped set. Control `run_011`. |
| LDG-012 activation gating | IMPLEMENTED+VERIFIABLE | `index.rs` activation bound to trusted qualification receipts + compatibility; `service.rs` `activate` fails closed (`ldg-windows.mjs:375` installed probe). Control `run_012`. |
| LDG-013 shadow comparison | IMPLEMENTED+VERIFIABLE | `doc_spine.rs:203-227` shadow path leaves active result unchanged. Control `run_013`. |
| LDG-014 bounded hierarchy reads | IMPLEMENTED+VERIFIABLE | `diagnostics.rs::related` — parent/child/sibling/breadcrumb over persisted ancestry, generation-bound. Control `run_014`. |
| LDG-015 link resolution | IMPLEMENTED+VERIFIABLE | `link_projection.rs` — inline/reference/autolink/relative/fragment/Unicode/broken link handling. Control `run_015`. |
| LDG-016 (EXCLUDED) bounded graph expansion | EXCLUDED-SAFEGUARDED | `graph_candidates` (`query.rs:226-288`) retains node/edge/byte caps + provenance receipt; atom excluded (`ledger.md:38`), no expansion of scope performed. |
| LDG-017 transactional publication | IMPLEMENTED+VERIFIABLE | `doc_spine.rs` generation-bound publication tuple; `run_017` (`qualification_lifecycle.rs:202`) — concurrent readers + coherent tuple + drift refusal. |
| LDG-018 incremental reconciliation | IMPLEMENTED+VERIFIABLE | `doc_spine.rs` reconcile; `run_018` — incremental rebuild, deletion tombstone, unchanged-root bounded work. |
| LDG-019 granted erasure | IMPLEMENTED+VERIFIABLE | `service.rs`/`erasure.rs` transactional projection erase + catalog-owned exclusion; `run_019` — durable fence, no resurrection. |
| LDG-020 deterministic document projection | IMPLEMENTED+VERIFIABLE | `doc_projection.rs:73-145`; `run_020` — projection + source-bound read. |
| LDG-021 (EXCLUDED) session projection non-recallable | EXCLUDED-SAFEGUARDED (hardened) | `session_projection.rs`: `SESSION_DOCUMENT_ID_PREFIX` (`:20`) + fail-closed write gate in `index_session_projection` (`:289-301`) — reserved `session-projection:` identity space, refusal if identity collides with `ledger_doc_artifacts`. Every recall/resolution lane joins `ledger_doc_artifacts`, so session projections cannot become ordinary documents. `run_021` (`:333`) preserved: separate owner cannot recall the projection. |
| LDG-022 Pull handoff candidates | IMPLEMENTED+VERIFIABLE; delivery dormant | `provider.rs::LedgerProvider` (`:10-117`) registered as `native.ledger` (`native_federation.rs:190-194`); grant-bound ticketed candidates with `membrane_source_read` resolver (`candidate_for_hit:234-259`). `run_022` covers materialization. Finding: automatic Pull delivery remains disabled/unqualified pending release-specific evidence — installed boundary not met. |
| LDG-023 (EXCLUDED) change refs/cursors | EXCLUDED-SAFEGUARDED | No journal/cursor mechanism exists or was added; `CASES` excludes `LDG-023` (`qualification_lifecycle.rs:726` test asserts absence). |
| LDG-024 alias/move history | IMPLEMENTED+VERIFIABLE | `doc_spine.rs`/`db.rs` alias transition history; `run_024` — move creates new source identity, old identity not retargeted; bounded `manifests` probe (`ldg-windows.mjs:412`). |
| LDG-025 (EXCLUDED) session projection | EXCLUDED-SAFEGUARDED (hardened) | Same gate as LDG-021 (`session_projection.rs:289-301`); `run_025` (`:392`) preserved — bounded scoped query, empty-grant denial. No session-store expansion performed. |
| LDG-026 confined canonical references | IMPLEMENTED+VERIFIABLE | `identifier.rs`/`service.rs`; canonical enrolled-root binding, traversal/symlink/duplicate-path/refused-grant denial; `run_026` + installed escape probe (`ldg-windows.mjs:336-340`). |
| LDG-027 (EXCLUDED) query aliases | EXCLUDED-SAFEGUARDED | `query_alias.rs` shadow alias projection retained source-bound and non-authoritative; `run_027` (`:442`) preserved — exact-first recall, stale alias refusal. Not reactivated. |
| LDG-028 granted conversion | IMPLEMENTED+VERIFIABLE | `document_conversion.rs`; raw/normalized hash binding, converter provenance, typed losses, snapshot-vs-live distinction; `run_028`. |
| LDG-029 inbound references | IMPLEMENTED+VERIFIABLE | `diagnostics.rs::backlinks` — scope-filtered generation-bound inbound edges with `authorityEffect:none`; `run_029` + installed arm (`ldg-windows.mjs:433`). |
| LDG-030 literal byte ranges | IMPLEMENTED+VERIFIABLE | `query.rs` literal lane — bounded eligible spans, exact byte verification, incomplete-search honesty; `run_030`. |
| LDG-031 structural manifests/drift | IMPLEMENTED+VERIFIABLE | `diagnostics.rs` bounded manifests + deterministic drift (diagnostic-only, erasable); `run_031` + installed arm (`ldg-windows.mjs:439`). |
| LDG-032 skill-document adapter | GAP-REPAIRED + PENDING-WIRE | New `skill_documents.rs` (635 lines): `skill_id_from_path` (`:48`), `catalog` (`:119`), `search` (`:212`), `document_hits`/`document_hits_granted` (`:287`/`:319`). Service APIs `skill_catalog`/`skill_documents`/`skill_document_hits` (`service.rs:217/239/272`) + `membrane_ledger` ops `skillCatalog`/`skillDocuments`/`skillDocument` (`service.rs:485-537`). `LedgerSkillProvider` emits `ProviderId::Skills` candidates with `membrane_source_read` binding (`provider.rs:131-232`). `query::document_hits` (`query.rs:313`) emits the document's top-level span partition — verified correct for headed `SKILL.md` (no `document` node exists for nonempty Markdown; parentless sections cover [0,len)). Native control `run_032` (`qualification_lifecycle.rs:605`). NOT YET EXECUTED and `native.skills` still points at the Cortex-backed provider — see PATCH REQUESTS. |

## CHANGES

All edits inside `engine/crates/membrane-runtime/src/ledger/`:

- `skill_documents.rs` (new, 635 lines) — LDG-032 adapter over existing
  registration/index/resolution. Adds no storage, no second index, no body
  copies: skill metadata is read from `ledger_doc_artifacts` rows; hits are
  ordinary `LedgerHit` values carrying the `membrane_source_read` binding.
  - `skill_id_from_path` (`:48`): only exact `tools/skills/<name>/SKILL.md`;
    rejects deeper paths, alternates filenames, empty/traversal/control/backslash
    names, oversized ids. Auxiliary files stay ordinary documents.
  - `catalog` (`:119`): eligibility mirrors recall gates — active,
    normal-sensitivity, policy-allowed, erasure-fence-checked; caller grant
    narrows enumeration to granted paths, never widens; `MAX_SKILL_DOCUMENTS`
    bound (512).
  - `search` (`:212`): the skill source set is expressed as grant-shaped
    narrowing into `query::search` — enrolled skills are the eligible set
    without a grant; a caller grant intersects, and an empty intersection is
    fail-closed (never widens into the general document set). A stray hit
    outside the catalog is recorded as an omission, not emitted.
  - `document_hits` (`:287`) / `document_hits_granted` (`:319`):
    catalog-selection materialization emits the complete top-level span set
    (`parent_id IS NULL` nodes — frontmatter/preamble/level-1 sections, or the
    `document` node for empty sources) so whole-file delivery is a bounded set
    of verifiable, ticket-resolvable spans, not a synthesized node. The granted
    variant mirrors `eligible`'s line→byte mapping and requires a grant to
    cover `[0, len)`; partial or wrong-path grants are refused
    (`ledger_skill_grant_excludes_document`). `imported` sources refuse
    (`snapshot_range_unsupported`).
  - Tests (`:362-635`): path convention, catalog filtering, search binding,
    grant narrow/never-widen, erasure removal, granted-coverage boundaries
    (whole/exact/partial/short/wrong-path/empty), source-and-span verification
    including drift refusal and the frontmatter+section partition.

- `query.rs` — `document_hits` (`:313-358`): materializes a document's
  parentless nodes as lane hits through the same `load_source` +
  span-hash-verification path as query lanes (`source_hit`); errors
  `ledger_document_spans_missing` when a registered doc has no top-level
  spans. This replaced an earlier whole-document design that assumed a
  `node_kind='document'` node — which `index.rs` creates only for empty
  Markdown — so headed `SKILL.md` files resolve through their real
  `frontmatter`/`preamble`/`section` spans.

- `service.rs` — `skill_catalog` (`:217`), `skill_documents` (`:239`),
  `skill_document_hits` (`:272`): each runs inside `LedgerService::run` with
  caller authorization, `sync_locked` reconciliation first, task-grant
  validation before and after, budget checks, owner-issued tickets per hit
  (`issue_ticket`). Operation arm `skillCatalog`/`skillDocuments`/
  `skillDocument` (`:485-537`) resolves an optional `scopeGrantId` through the
  existing catalog grant lookup (typed `ReadPathV1` ranges), requiring
  `taskId`+`sessionId` binding when a grant is present — identical grant
  posture to `recall`. Reachable through the existing `membrane_ledger` MCP
  arm (`mcp_executor.rs:1845-1852` passes `operation` through; no surface
  change needed).

- `provider.rs` — `LedgerSkillProvider` (`:131-202`) implementing `Provider`
  for `ProviderId::Skills`: same cancel/deadline/`spawn_blocking`/`CancelWork`
  discipline as `LedgerProvider`; `skill_materialize` (`:170`) calls
  `owner.skill_documents` and wraps each ticketed hit via
  `candidate_for_skill` (`:207`) — `membrane_source_read` resolver,
  `skills:<skill_id>` id, `workspace_tracked` trust, `instruction_policy:
  data_only`, `exact`/`recoverable` true, base revision + overlay content
  hash + `ledger:<generation>` snapshot identity. Provider output reports
  `provenance: ledger-source-owner`, completeness, omissions. Marked
  `#[allow(dead_code)]` — production registration is outside `ledger/**`
  (PR-2). Test `skill_candidate_carries_source_read_binding_not_cortex_resolver`
  (`:289`) pins the binding and the absence of a Cortex resolver.

- `session_projection.rs` — LDG-021/025 fail-closed write gate:
  `SESSION_DOCUMENT_ID_PREFIX` (`:20`) and `index_session_projection`
  (`:289-301`) now refuse a document id outside the reserved
  `session-projection:` space and any id already present in
  `ledger_doc_artifacts` (`SessionProjectionError::RegisteredSource`).
  Session projections may write only to the derived `parent_doc_id`-keyed
  projection store, which no recall/resolution lane reads — the boundary
  keeps them non-recallable while retired rows exist.

- `qualification_lifecycle.rs` — `LDG-032` added to `CASES` (`:25`), source
  marker (`:120`), dispatch (`:147`), and `run_032` (`:605-710`): registers
  `tools/skills/deploy/SKILL.md`, asserts the catalog surfaces exactly that
  skill, asserts search hits carry source-ref/span-hash/content-hash/
  revision/generation, proves grant narrowing is fail-closed, proves the
  top-level span set covers `[0, len)` with no fabricated `document` node,
  proves whole-grant admission + partial-grant refusal, drift refusal before
  ticketing, and erasure-fence removal from catalog and search.
  `run_installed` (`:157-200`) unchanged: `evidenceKind` upgrades to
  `installed_native` only under an installer-owned `current` root with
  `release.json` version + executable SHA evidence.

- `mod.rs` — `pub(crate) mod skill_documents;` (`:31`).

No new tables, no new storage authority, no new public tool; the only new
`membrane_ledger` operations reuse the existing owner operation dispatch.

## VERIFICATION PERFORMED

- `pnpm --dir apps/membrane-hub run release:validate:win` — authorized
  local-dev lane; runs `rightkit cargo check --manifest-path
  ../../engine/Cargo.toml --workspace --all-targets --locked --target
  x86_64-pc-windows-msvc` (phase `engine-source-validation`, 948s). Result:
  `membrane-runtime` lib failed on **one unrelated error**
  (`hook_diagnostics.rs:198` — `Option<()>` vs
  `Option<RemainingContextCeilingV1>`, another lane's in-flight change);
  lib test failed on 3 errors including one lane-owned defect this run
  caught and fixed (`provider.rs:299` — `super::skill_documents` →
  `super::super::skill_documents` inside `mod tests`).
- Re-run `pnpm exec rightkit cargo check --manifest-path
  ../../engine/Cargo.toml -p membrane-runtime --all-targets --locked
  --target x86_64-pc-windows-msvc` (from `apps/membrane-hub`, the lane's
  permitted cwd) after the fix: `membrane-runtime` lib = 1 error
  (`hook_diagnostics.rs:198`), lib test = 2 errors (`hook_diagnostics.rs:198`,
  `residency.rs:363` — both unrelated, owned by other lanes).
  **Zero errors or warnings in `ledger/**`** across lib + all targets.
- `git diff` review of every lane file: renamed symbols consistent
  (`document_hits`/`document_hits_granted`/`skill_document_hits`); borrow
  discipline (statement dropped before connection guard in `catalog` and
  `document_hits`); `usize` span types match `LedgerHit`; grant coverage
  mapping mirrors `eligible`'s line→byte semantics.
- **NOT run** (blocked, see BLOCKERS): `cargo test`, `qualification ledger
  LDG-032` execution, `run_032` and all new unit tests, and the installed
  unsigned-installer route (`release:local:win:unsigned`) — all require a
  compiling lib, which the unrelated `hook_diagnostics.rs:198` error
  prevents.

## PATCH REQUESTS

Outside owned paths; for the coordinator/owning lanes.

### PR-1 — register `LDG_032` in the installed case registry (qualification lane)

`scripts/qualification/cases/ldg-windows.mjs` exports `LDG_001..LDG_031`
(`:891-922`); there is no `LDG_032` export, so the installed runner
(`run.mjs --group LDG`) never invokes `qualification ledger LDG-032`. The
native case exists and is dispatchable today
(`cli.rs:5415-5433` → `qualification_lifecycle::run`). Add, mirroring
`LDG_031`:

```js
export function LDG_032(context = {}) { const id = 'LDG-032'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
```

plus a `cases.LDG_032` entry if a source-structural row is wanted. The
existing `registryOutcome` already binds native + installed identity
evidence (`:224-244`); no new plumbing is needed. Note the dead code at
`:245-451` (unreachable per-id arms after the early `return`) predates this
lane — left untouched.

### PR-2 — point `native.skills` at the Ledger skill provider (Pull/native wiring owner)

`pull/native_federation.rs:173-183` registers `ProviderId::Skills` with
`SkillsProvider::new(bindings.skills…)`, backed by `RuntimeSkillsSource`
(Cortex store, `pull/federation_sources.rs:34,139`). The Ledger-side
provider is ready and test-pinned:
`crate::ledger::provider::LedgerSkillProvider::new(bindings.ledger.clone())`
takes the same `Option<Arc<LedgerService>>` binding `LedgerProvider` already
uses (`native_federation.rs:190-194`). Options for the owning lane:

1. Replace the `native.skills` provider with `LedgerSkillProvider` so
   `ProviderId::Skills` emits source-bound `membrane_source_read` candidates
   (the LDG-032 / LDG-D018 target: Ledger owns the authoritative
   skill-body index; separately admitted durable skill insights stay a
   Cortex lane), or
2. Register `LedgerSkillProvider` alongside under a distinct provider id if
   Pull wants both lanes during migration.

Until this lands, `LedgerSkillProvider` compiles as `#[allow(dead_code)]`
and Pull's skill lane remains Cortex-backed — LDG-032's "deliver bounded
exact resolver results to Pull" boundary is not production-complete.

## BLOCKERS

1. **Workspace does not compile; no test/qualification execution possible.**
   `membrane-runtime` lib fails on `hook_diagnostics.rs:198`
   (`Option<()>` → `Option<RemainingContextCeilingV1>` for
   `hook_mode_federate_with_observation`); lib test additionally fails on
   `residency.rs:363` (`identity` fn item vs value). Both files are owned by
   other in-flight lanes — not touched here. Until they compile, `run_032`,
   all new `skill_documents.rs`/`provider.rs` unit tests, and
   `qualification ledger <id>` cannot execute; every new control is
   compile-clean but execution-unverified.
2. **No installed evidence.** The installer-owned `current` root cannot be
   produced while the workspace fails to build; `run_installed`
   (`qualification_lifecycle.rs:157`) correctly refuses to label evidence
   installed otherwise. No atom is claimed RELEASED.
3. **LDG-022 delivery remains dormant.** Candidate materialization is wired
   (`native.ledger` registered), but automatic Pull delivery is
   disabled/unqualified pending release-specific evidence — unchanged by
   this lane, restated for the atom table.
4. **LDG-032 production wiring pending** — PR-2 above. The owned adapter
   and provider exist and are compile-verified; Pull's live skill lane still
   resolves through the Cortex-backed `SkillsProvider` until the external
   registration change lands.

Next dependency: (a) owning lanes fix `hook_diagnostics.rs:198` /
`residency.rs:363` so `cargo test -p membrane-runtime` and
`membrane qualification ledger LDG-032` can execute; (b) PR-1 registers the
installed case row; (c) PR-2 repoints `native.skills`; (d)
`pnpm run release:local:win:unsigned` then produces the installer-owned
`current` root for `run_installed` + delivery evidence.

<!-- reconcile:start -->

## Reconciliation

Material revision: `0c326b31a6c7b4803a590d7d6ca951d203c50da0`. Exact source/consumer locators verified against this revision.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| LDG-001 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs`; `engine/crates/membrane-runtime/src/ledger/policy.rs`; `engine/crates/membrane-runtime/src/ledger/reconcile.rs`; `engine/crates/membrane-runtime/src/ledger/service.rs` | `engine/crates/membrane-runtime/src/ledger/qualification_core.rs` | COMPLETE |
| LDG-002 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/index.rs`; `engine/crates/membrane-runtime/src/ledger/outline.rs` | `engine/crates/membrane-runtime/src/ledger/outline.rs` | COMPLETE |
| LDG-003 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/index.rs`; `engine/crates/membrane-runtime/src/ledger/db.rs` | `engine/crates/membrane-runtime/src/ledger/db.rs` | COMPLETE |
| LDG-004 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/index.rs`; `engine/crates/membrane-runtime/src/ledger/outline.rs`; `engine/crates/membrane-runtime/src/ledger/doc_spine.rs` | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs` | COMPLETE |
| LDG-005 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/resolve.rs:219-301`; `engine/crates/membrane-runtime/src/ledger/resolve.rs` | `engine/crates/membrane-runtime/src/ledger/resolve.rs` | COMPLETE |
| LDG-006 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/resolve.rs`; `engine/crates/membrane-runtime/src/ledger/outline.rs`; `engine/crates/membrane-runtime/src/ledger/service.rs` | `engine/crates/membrane-runtime/src/ledger/service.rs` | COMPLETE |
| LDG-007 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/index.rs:512-590`; `engine/crates/membrane-runtime/src/ledger/index.rs` | `engine/crates/membrane-runtime/src/ledger/index.rs` | COMPLETE |
| LDG-008 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/query.rs` | — | COMPLETE |
| LDG-009 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/index.rs` | — | COMPLETE |
| LDG-010 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/db.rs:50-95`; `engine/crates/membrane-runtime/src/ledger/index.rs:153-375` | `engine/crates/membrane-runtime/src/ledger/index.rs:153-375` | COMPLETE |
| LDG-011 | DELIVERED | — | — | COMPLETE |
| LDG-012 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/index.rs`; `engine/crates/membrane-runtime/src/ledger/service.rs` | `engine/crates/membrane-runtime/src/ledger/service.rs` | COMPLETE |
| LDG-013 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs:203-227` | — | COMPLETE |
| LDG-014 | DELIVERED | — | — | COMPLETE |
| LDG-015 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/link_projection.rs` | — | COMPLETE |
| LDG-017 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs` | `engine/crates/membrane-runtime/src/ledger/qualification_lifecycle.rs:202` | COMPLETE |
| LDG-018 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs` | — | COMPLETE |
| LDG-019 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/service.rs`; `engine/crates/membrane-runtime/src/ledger/erasure.rs` | `engine/crates/membrane-runtime/src/ledger/erasure.rs` | COMPLETE |
| LDG-020 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_projection.rs:73-145` | — | COMPLETE |
| LDG-024 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs`; `engine/crates/membrane-runtime/src/ledger/db.rs` | `engine/crates/membrane-runtime/src/ledger/db.rs` | COMPLETE |
| LDG-026 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/identifier.rs`; `engine/crates/membrane-runtime/src/ledger/service.rs` | `engine/crates/membrane-runtime/src/ledger/service.rs` | COMPLETE |
| LDG-028 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/document_conversion.rs` | — | COMPLETE |
| LDG-029 | DELIVERED | — | — | COMPLETE |
| LDG-030 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/query.rs` | — | COMPLETE |
| LDG-031 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/diagnostics.rs` | — | COMPLETE |

<!-- reconcile:end -->
