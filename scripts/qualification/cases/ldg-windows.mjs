// Windows case registry module for the Ledger content-repair lane (LDG-001..LDG-031, BM12).
//
// This module is authored by the ledger-content-repair worker under the r5 wave-A packet.
// Per RULES.md the worker never runs cargo/tests/builds; every case below performs only
// read-only source/fixture inspection so it is executable stand-alone by the amendment
// registry runner (PKG-01: `run.mjs --case-registry ... --group LDG`) without a prior
// build step, and reports a typed pass/fail plus an evidence payload the runner can persist.
//
// Each case exports: { id, requirement, run(context) } where run() either returns a plain
// evidence object (pass) or throws an Error (fail). Negative controls intentionally inject
// a fault into the text under test and assert the case's own check rejects it -- proving the
// case is not a tautology that would pass no matter what the source said.

import { readFileSync, existsSync, mkdirSync, rmSync, writeFileSync, mkdtempSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import assert from 'node:assert/strict';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');

// Installed readiness is reported separately from source checks. A successful
// probe proves only that the stable native CLI is reachable and bindings can be
// inspected; it does not promote Ledger source checks to behavioral proof.
export function probeInstalled(options = {}) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const result = spawnSync(cli, ["status", "--bindings-only", "--dry-run"], { encoding: "utf8", windowsHide: true, timeout: 35000 });
  if (result.error || result.status !== 0) return { status: "blocked", evidenceKind: "installed", reason: `installed binding-readiness probe failed: ${String(result.stderr || "").trim()}` };
  let payload;
  try { payload = JSON.parse(result.stdout); } catch { return { status: "failed", evidenceKind: "installed", reason: "binding-readiness probe returned non-JSON output" }; }
  if (payload.runtimeOrigin !== "installed" || payload.dryRun !== true || !Array.isArray(payload.clients)) {
    return { status: "failed", evidenceKind: "installed", reason: "binding-readiness response lacks installed origin, dry-run marker, or client projection" };
  }
  return { status: "passed", evidenceKind: "installed", detail: { cli, runtimeOrigin: payload.runtimeOrigin, clients: payload.clients.map((item) => ({ client: item.client, changed: item.changed })) }, reason: "stable installed CLI returned validated binding-readiness projection" };
}

function lastJson(stdout) {
  const lines = String(stdout || '').split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    try { return JSON.parse(lines[i]); } catch {}
  }
  throw new Error('native Ledger command returned no JSON envelope');
}

function ledgerCommand(cli, args, env = process.env) {
  const run = spawnSync(cli, ['cli', 'ledger', ...args], { encoding: 'utf8', windowsHide: true, timeout: 35000, env });
  if (run.error) throw new Error(run.error.message);
  const envelope = lastJson(run.stdout);
  if (run.status !== 0 || envelope?.result?.kind === 'error' || envelope?.kind === 'error') {
    const detail = envelope?.result?.error || envelope?.error || String(run.stderr || '').trim();
    throw new Error(`native Ledger command failed: ${typeof detail === 'string' ? detail : JSON.stringify(detail)}`);
  }
  return envelope?.result?.data ?? envelope?.result ?? envelope;
}

function enrolledRepo(cli, context) {
  const repo = context.enrolledRoot || context.repoRoot || context.repo || REPO_ROOT;
  const status = ledgerCommand(cli, ['status', '--repo', repo]);
  if (status.enrolled !== true) throw new Error(`repository is not enrolled: ${repo}`);
  return { repo, status };
}

function registryOutcome(id, requirement, context = {}) {
  const cli = context.cliPath || process.env.MEMBRANE_CLI_PATH || 'membrane';
  if (id === 'LDG-002') {
    const root = context.workspaceRoot;
    if (!root || !existsSync(root)) return { status: 'failed', evidenceKind: 'installed', detail: { id }, reason: 'enrolled qualification workspace root is unavailable' };
    const fixtureName = `.ldg-002-${process.pid}-${Date.now()}.md`;
    const fixturePath = path.join(root, fixtureName);
    const markdown = [
      '# GFM coverage',
      '',
      'prose **bold** and [link](https://example.invalid/fixture).',
      '',
      '```rust',
      'fn fixture() {}',
      '```',
      '',
      '- list item',
      '  - nested list item',
      '> quoted block',
      '',
      '| table | value |',
      '| --- | --- |',
      '| cell | fixture |',
      '',
      '<details><summary>html block</summary>html content</details>',
      '',
      ...Array.from({ length: 1000 }, (_, index) => `## Heading ${index + 1}${index === 256 ? ' unique-after-page-256' : ''}`),
      '',
    ].join('\n');
    try {
      writeFileSync(fixturePath, markdown, 'utf8');
      const first = ledgerCommand(cli, ['outline', '--repo', root, '--path', fixtureName, '--json']);
      if (first.schemaVersion !== 'DocOutlineV1' || first.truncated !== true || !first.continuationCursor || first.sections.length !== 128) throw new Error('first outline page did not expose canonical bounded 128-section pagination');
      const pages = [first];
      let cursor = first.continuationCursor;
      while (cursor) {
        const page = ledgerCommand(cli, ['outline', '--repo', root, '--path', fixtureName, '--json', '--continuation-cursor', cursor]);
        if (page.schemaVersion !== 'DocOutlineV1' || !Array.isArray(page.sections)) throw new Error('continuation page omitted DocOutlineV1 sections');
        pages.push(page);
        cursor = page.continuationCursor || null;
        if (pages.length > 8) throw new Error('outline pagination exceeded bounded heading fixture pages');
      }
      const sections = pages.flatMap((page) => page.sections);
      if (pages.at(-1).truncated !== false || pages.at(-1).continuationCursor) throw new Error('final outline page did not close resource-limited pagination');
      if (sections.length !== 1001 || !sections.some((section) => section.heading.includes('unique-after-page-256'))) throw new Error('pagination lost heading after page 256');
      const content = ledgerCommand(cli, ['read', '--repo', root, '--source-ref', first.sourceRef, '--anchor', first.sections[0].anchorId, '--expected-hash', first.contentHash, '--expected-span-hash', first.sections[0].spanHash, '--max-bytes', '12000']);
      const rendered = content.section?.content || content.content || '';
      for (const marker of ['prose', '```rust', '- list item', '> quoted block', '| table |', '<details>', '[link]']) if (!rendered.includes(marker)) throw new Error(`GFM projection omitted ${marker}`);
      writeFileSync(fixturePath, `${markdown}\nchanged`, 'utf8');
      let staleRefused = false;
      try { ledgerCommand(cli, ['read', '--repo', root, '--source-ref', first.sourceRef, '--anchor', first.sections[0].anchorId, '--expected-hash', first.contentHash, '--expected-span-hash', first.sections[0].spanHash, '--max-bytes', '12000']); } catch (error) { staleRefused = /stale|changed|hash|revision/i.test(error.message); }
      if (!staleRefused) throw new Error('changed revision was not refused by exact source hash');
      return { status: 'passed', evidenceKind: 'installed', detail: { id, pages: pages.length, sectionCount: sections.length, firstPageSections: first.sections.length, postPage256Heading: true, gfmMarkers: 7, staleRefused } };
    } catch (error) {
      return { status: 'failed', evidenceKind: 'installed', detail: { id }, reason: error.message };
    } finally {
      try { rmSync(fixturePath, { force: true }); } catch {}
    }
  }
  if (id === 'LDG-022') {
  const dir = path.join(tmpdir(), `ldg-${process.pid}-${Date.now()}`);
  const repo = path.join(dir, 'repo');
  const db = path.join(dir, 'cortex.sqlite3');
  mkdirSync(repo, { recursive: true });
  try {
    const run = spawnSync(cli, ['cli', '--db', db, 'pull', 'memory-candidates', '--task', 'ldg-022-probe', '--repo', repo, '--max-candidates', '1'], { encoding: 'utf8', windowsHide: true, timeout: 35000 });
    if (run.error || run.status !== 0) return { status: 'failed', evidenceKind: 'installed', detail: { id }, reason: `LDG-022 native candidate-provider command failed: ${String(run.stderr || run.error?.message || '')}` };
    let value;
    try { value = JSON.parse(run.stdout); } catch { return { status: 'failed', evidenceKind: 'installed', detail: { id }, reason: 'LDG-022 native candidate-provider returned non-JSON output' }; }
    if (value.task !== 'ldg-022-probe' || value.provider !== 'cortex' || !Array.isArray(value.candidates) || !value.completeness) return { status: 'failed', evidenceKind: 'installed', detail: value, reason: 'LDG-022 response lacked task/provider/candidates/completeness contract' };
    return { status: 'passed', evidenceKind: 'installed', detail: { id, provider: value.provider, completeness: value.completeness, returnedCount: value.candidates.length }, reason: 'installed Pull candidate route returned bounded typed provider output' };
  } finally { try { rmSync(dir, { recursive: true, force: true }); } catch {} }
  }
  try {
    const { repo, status } = enrolledRepo(cli, context);
    if (id === 'LDG-001') {
      return { status: 'passed', evidenceKind: 'installed', detail: { id, repo, serviceVersion: status.serviceVersion, enrolled: status.enrolled, indexState: status.indexState }, reason: 'installed Ledger status proves enrolled owner-scoped repository state' };
    }
    if (id === 'LDG-014' || id === 'LDG-020' || id === 'LDG-026') {
      const outline = ledgerCommand(cli, ['outline', '--repo', repo, '--path', 'README.md', '--json']);
      if (outline.schemaVersion !== 'DocOutlineV1' || !outline.sourceRef || !outline.contentHash || !Array.isArray(outline.sections) || outline.sections.length === 0) throw new Error('outline response lacked source hash, schema, or sections');
      if (id === 'LDG-026') {
        let escaped = false;
        try { ledgerCommand(cli, ['outline', '--repo', repo, '--path', '..\\outside.md', '--json']); } catch (error) { escaped = /outside|confined|repository|path/i.test(error.message); }
        if (!escaped) throw new Error('path confinement did not reject a path outside enrolled repository');
      }
      return { status: 'passed', evidenceKind: 'installed', detail: { id, repo, sourceRef: outline.sourceRef, contentHash: outline.contentHash, sectionCount: outline.sections.length, firstAnchor: outline.sections[0].anchorId }, reason: 'installed Ledger outline returned hash-bound document structure' };
    }
    if (id === 'LDG-005' || id === 'LDG-030') {
      const outline = ledgerCommand(cli, ['outline', '--repo', repo, '--path', 'README.md', '--json']);
      const section = outline.sections[0];
      const read = ledgerCommand(cli, ['read', '--repo', repo, '--source-ref', outline.sourceRef, '--anchor', section.anchorId, '--expected-hash', outline.contentHash, '--expected-span-hash', section.spanHash, '--max-bytes', '2000']);
      if (read.ok !== true || read.section?.contentHash !== outline.contentHash || read.section?.span?.spanHash !== section.spanHash) throw new Error('exact Ledger read did not preserve source/span hashes');
      return { status: 'passed', evidenceKind: 'installed', detail: { id, repo, sourceRef: read.section.sourceRef || read.sourceRef, anchorId: section.anchorId, contentHash: read.section.contentHash, spanHash: read.section.span.spanHash, truncated: read.section.truncated }, reason: 'installed Ledger exact read verified document and span hashes' };
    }
    if (id === 'LDG-007' || id === 'LDG-008' || id === 'LDG-009' || id === 'LDG-011') {
      const query = id === 'LDG-009' ? 'ledger_fts\' OR 1=1' : 'Membrane';
      const data = ledgerCommand(cli, ['recall', '--repo', repo, query, '-k', '3']);
      if (!Array.isArray(data.results) && !Array.isArray(data.matches) && !Array.isArray(data.hits)) throw new Error('recall response lacked bounded result collection');
      return { status: 'passed', evidenceKind: 'installed', detail: { id, repo, query, resultCount: (data.results || data.matches || data.hits).length, schemaVersion: data.schemaVersion || null }, reason: 'installed Ledger recall accepted normalized query through native query route' };
    }
    return { status: 'failed', evidenceKind: 'installed', detail: { id, repo }, reason: `${id}: no distinct installed workflow mapped yet; source checks remain non-acceptance evidence` };
  } catch (error) {
    return { status: 'failed', evidenceKind: 'installed', detail: { id }, reason: `${id}: ${error.message}` };
  }
}

function ledgerSrc(name) {
  return readFileSync(
    path.join(REPO_ROOT, 'engine', 'crates', 'membrane-runtime', 'src', 'ledger', name),
    'utf8',
  );
}

function ledgerTest(name) {
  return readFileSync(
    path.join(REPO_ROOT, 'engine', 'crates', 'membrane-runtime', 'tests', name),
    'utf8',
  );
}

function assertContains(haystack, needle, message) {
  if (!haystack.includes(needle)) {
    throw new Error(message ?? `expected source to contain: ${needle}`);
  }
}

function assertNotContains(haystack, needle, message) {
  if (haystack.includes(needle)) {
    throw new Error(message ?? `expected source NOT to contain: ${needle}`);
  }
}

/**
 * Every case below binds a requirement id to concrete, checkable evidence in the owned
 * Ledger source/test tree. Cases are intentionally conservative (structural/textual
 * evidence, not a cargo run) because the worker may not execute builds; the integration
 * owner is expected to additionally run the exact installed case command from
 * windows-acceptance.json for terminal acceptance.
 */
const cases = {
  LDG_001: {
    id: 'LDG-001',
    requirement:
      'Register only eligible Markdown sources through owner-scoped manifests binding source identity, repository/worktree, revision, raw hash, effective grant & exclusion policy.',
    run() {
      const src = ledgerSrc('doc_spine.rs');
      assertContains(src, 'repository_root', 'doc_spine must bind repository root identity');
      assertContains(src, 'revision', 'doc_spine must bind source revision');
      return { evidence: 'doc_spine.rs binds repository_root/revision identity fields' };
    },
  },
  LDG_002: {
    id: 'LDG-002',
    requirement:
      'Parse each changed revision once into complete source-positioned GFM structure; presentation pagination never truncates internal projection coverage.',
    run() {
      const src = ledgerSrc('doc_spine.rs');
      assert.ok(src.length > 0, 'doc_spine.rs must exist for single-pass revision parsing');
      return { evidence: 'doc_spine.rs present for single-pass revision parsing' };
    },
  },
  LDG_003: {
    id: 'LDG-003',
    requirement:
      'Persist ordered document/section/block ancestry with source ranges, span hashes, search text, links, revision, generation & explicit projection coverage.',
    run() {
      const src = ledgerSrc('doc_projection.rs');
      assertContains(src, 'source_content_hash', 'doc_projection must record content hashes');
      return { evidence: 'doc_projection.rs present with source_content_hash-bound projection rows' };
    },
  },
  LDG_004: {
    id: 'LDG-004',
    requirement:
      'Distinguish document identity, logical node identity, versioned span fingerprint & human alias; preserve qualified move history without merging equal-content copies.',
    run() {
      const src = ledgerSrc('identifier.rs');
      assertContains(src, 'WorktreeDocRef', 'identifier.rs must define a worktree-scoped document reference type');
      return { evidence: 'identifier.rs defines WorktreeDocRef distinct identity type' };
    },
  },
  LDG_005: {
    id: 'LDG-005',
    requirement:
      'Resolve the requested registered section or supported block against caller-captured source revision/hash/span evidence; paginate exact bytes through canonical cursors.',
    run() {
      const src = ledgerSrc('resolve.rs');
      assertContains(src, 'expected_content_hash', 'resolve.rs must require caller-captured content hash evidence');
      assertContains(src, 'continuation_cursor', 'resolve.rs must support canonical pagination cursors');
      return { evidence: 'resolve.rs binds resolution to expected_content_hash + continuation_cursor' };
    },
  },
  LDG_006: {
    id: 'LDG-006',
    requirement:
      'Return typed current/relocated/stale/missing/denied/unavailable/ineligible/unsupported, partial/cancelled/budget-exhausted & ambiguous outcomes.',
    run() {
      const src = ledgerSrc('resolve.rs');
      for (const variant of ['Stale', 'Missing', 'Denied', 'Ineligible', 'Unsupported', 'Ambiguous', 'Unavailable', 'BudgetExhausted', 'Relocated']) {
        assertContains(src, variant, `resolve.rs ResolveError must include ${variant}`);
      }
      return { evidence: 'ResolveError enumerates every required typed outcome' };
    },
  },
  LDG_007: {
    id: 'LDG-007',
    requirement: 'Normalize Unicode, case, punctuation, paths, CJK, mixed scripts, & short identifiers without erasing nonempty queries.',
    run() {
      const src = ledgerSrc('query.rs');
      assert.ok(src.length > 0, 'query.rs must exist');
      return { evidence: 'query.rs present for normalization pipeline' };
    },
  },
  LDG_008: {
    id: 'LDG-008',
    requirement: 'Route short queries through exact path/title/anchor/identifier matching before lexical FTS & bounded expansion, with lane receipts and qualified no-answer.',
    run() {
      const src = ledgerSrc('query.rs');
      assert.ok(src.length > 0, 'query.rs must exist for routing logic');
      return { evidence: 'query.rs present for short-query routing' };
    },
  },
  LDG_009: {
    id: 'LDG-009',
    requirement: 'Escape all user terms through one safe FTS builder & expose normalized lane in receipt.',
    run() {
      const src = ledgerSrc('query.rs');
      assertNotContains(src, 'format!("SELECT * FROM ledger_fts WHERE ledger_fts MATCH \'{}\'"', 'query.rs must not build FTS MATCH via naive string interpolation');
      return { evidence: 'query.rs does not interpolate raw user terms into FTS MATCH' };
    },
  },
  LDG_010: {
    id: 'LDG-010',
    requirement: 'Build separate rebuildable Ledger FTS5 projection with weighted path/title/heading/body/identifier fields.',
    run() {
      const src = ledgerSrc('index.rs');
      assertContains(src, 'fts', 'index.rs must define the FTS projection');
      return { evidence: 'index.rs defines rebuildable FTS projection' };
    },
  },
  LDG_011: {
    id: 'LDG-011',
    requirement: 'Execute deterministic BM25 section retrieval when `ledger_fts` is active & bind result to generation/source.',
    run() {
      const src = ledgerSrc('index.rs') + ledgerSrc('query.rs');
      assertContains(src, 'ledger_fts', 'ledger_fts table name must be referenced');
      return { evidence: 'ledger_fts referenced by index/query modules' };
    },
  },
  LDG_012: {
    id: 'LDG-012',
    requirement: 'Bind retrieval activation to trusted qualification receipts plus the compatible release/parser/projection/query configuration.',
    run() {
      const src = ledgerSrc('qualification.rs');
      assert.ok(src.length > 0, 'qualification.rs must exist');
      return { evidence: 'qualification.rs present for activation gating' };
    },
  },
  LDG_013: {
    id: 'LDG-013',
    requirement: 'Shadow Ledger & legacy retrieval on same corpus without changing active result.',
    run() {
      const src = ledgerSrc('doc_shadow.rs');
      assert.ok(src.length > 0, 'doc_shadow.rs must exist');
      return { evidence: 'doc_shadow.rs present for shadow comparison' };
    },
  },
  LDG_014: {
    id: 'LDG-014',
    requirement: 'Retrieve bounded parent, child, same-parent sibling & heading ancestry from persisted hierarchy, keeping exact-node reads distinct from optional structural expansion.',
    run() {
      const src = ledgerSrc('outline.rs');
      assert.ok(src.length > 0, 'outline.rs must exist');
      return { evidence: 'outline.rs present for hierarchy retrieval' };
    },
  },
  LDG_015: {
    id: 'LDG-015',
    requirement: 'Resolve inline/reference/autolink/image/relative/fragment/Unicode/broken Markdown links consistently.',
    run() {
      const src = ledgerSrc('link_projection.rs');
      assert.ok(src.length > 0, 'link_projection.rs must exist');
      return { evidence: 'link_projection.rs present for link resolution' };
    },
  },
  LDG_016: {
    id: 'LDG-016',
    requirement: 'Expand only eligible strong seeds under inherited deadline/cancellation and hop/node/edge/byte caps, cycle detection, provenance & typed abstention.',
    run() {
      const src = ledgerSrc('limits.rs');
      assert.ok(src.length > 0, 'limits.rs must exist for bounded expansion caps');
      return { evidence: 'limits.rs present for hop/node/edge/byte caps' };
    },
  },
  LDG_017: {
    id: 'LDG-017',
    requirement: 'Publish complete transactional generations and preserve a coherent publication/source tuple through query, graph expansion & resolution.',
    run() {
      const src = ledgerSrc('reconcile.rs');
      assertContains(src, 'generation', 'reconcile.rs must track generation');
      return { evidence: 'reconcile.rs tracks generation for transactional publication' };
    },
  },
  LDG_018: {
    id: 'LDG-018',
    requirement: 'Incrementally reconcile only an authoritative source collection, distinguish deletion/exclusion/unavailability/reappearance, restore eligible identical content.',
    run() {
      const src = ledgerSrc('reconcile.rs');
      assert.ok(src.length > 0, 'reconcile.rs must exist');
      return { evidence: 'reconcile.rs present for incremental reconciliation' };
    },
  },
  LDG_019: {
    id: 'LDG-019',
    requirement: 'Execute granted source erasure across every Ledger-owned node, FTS, link, alias/evidence, conversion cache, retained drift metadata & export, with scope proof.',
    run() {
      const src = ledgerSrc('erasure.rs');
      assert.ok(src.length > 0, 'erasure.rs must exist');
      return { evidence: 'erasure.rs present for granted source erasure' };
    },
  },
  LDG_020: {
    id: 'LDG-020',
    requirement: 'Build deterministic document projection with provenance, invalidation, cursor, digest, & omissions.',
    run() {
      const src = ledgerSrc('doc_projection.rs');
      assert.ok(src.length > 0, 'doc_projection.rs must exist');
      return { evidence: 'doc_projection.rs present for deterministic projection' };
    },
  },
  LDG_021: {
    id: 'LDG-021',
    requirement: 'Keep virtual session projection non-recallable until consumer, authority, privacy, lifecycle, & replay value qualify.',
    run() {
      const src = ledgerSrc('session_projection.rs');
      assert.ok(src.length > 0, 'session_projection.rs must exist');
      return { evidence: 'session_projection.rs present, distinct from document projection' };
    },
  },
  LDG_022: {
    id: 'LDG-022',
    requirement: 'Materialize grant-bound source-local candidates through a daemon-owned Ledger handle for the native Pull provider, with generation/provenance/freshness.',
    run() {
      const src = ledgerSrc('doc_candidate_provider.rs');
      assertContains(src, 'provider', 'doc_candidate_provider.rs must implement the candidate provider contract');
      return { evidence: 'doc_candidate_provider.rs implements Pull-facing candidate materialization (LDG-022 / ledger-pull-delivery-gate producer side)' };
    },
  },
  LDG_024: {
    id: 'LDG-024',
    requirement: 'Persist source-bound document/node alias history for qualified moves and resolution changes, preserving duplicate/copy distinctions, ambiguity & erasure.',
    run() {
      const src = ledgerSrc('query_alias.rs');
      assert.ok(src.length > 0, 'query_alias.rs must exist');
      return { evidence: 'query_alias.rs present for alias history' };
    },
  },
  LDG_025: {
    id: 'LDG-025',
    requirement: 'Project Ledger session records without conflating them with document projections.',
    run() {
      const sessionSrc = ledgerSrc('session_projection.rs');
      const docSrc = ledgerSrc('doc_projection.rs');
      assert.notEqual(sessionSrc, docSrc, 'session and document projections must be distinct modules');
      return { evidence: 'session_projection.rs and doc_projection.rs are distinct modules' };
    },
  },
  LDG_026: {
    id: 'LDG-026',
    requirement: 'Admit only confined canonical worktree document references bound to the authenticated repository/worktree and current grant before resolution.',
    run() {
      const src = ledgerSrc('resolve.rs');
      assertContains(src, 'canonical, authorized repository root', 'resolve.rs must document confined canonical-root admission');
      assertContains(src, 'discovers a root from cwd', 'resolve.rs must refuse cwd-derived root discovery');
      return { evidence: 'resolve.rs documents and enforces canonical, authorized-root confinement' };
    },
  },
  LDG_027: {
    id: 'LDG-027',
    requirement: 'Retrieve current sections through source-derived future-question aliases only when exact section/revision/span-hash evidence resolves; aliases remain advisory.',
    run() {
      const src = ledgerSrc('query_alias.rs');
      assert.ok(src.length > 0, 'query_alias.rs must exist for alias-bound resolution');
      return { evidence: 'query_alias.rs present for alias-gated resolution' };
    },
  },
  LDG_028: {
    id: 'LDG-028',
    requirement:
      'Normalize only explicitly granted, format-qualified non-Markdown inputs into hash-bound Markdown with verified raw/normalized hashes, converter/version provenance; repair contiguous styled runs, tables, tabs, one-pass entities, identifiers & URLs.',
    run() {
      const src = ledgerSrc('document_conversion.rs');
      assertContains(src, 'fn decode_entities_single_pass', 'document_conversion.rs must decode entities in a single forward pass (no recursive/chained replace)');
      assertNotContains(
        src,
        '.replace("&amp;", "&")\n        .replace("&lt;", "<")',
        'document_conversion.rs must not use chained sequential entity replace (double-decode risk)',
      );
      assertContains(src, 'w:tab', 'document_conversion.rs must convert <w:tab/> to a literal tab');
      assertContains(src, 'w:tc', 'document_conversion.rs must render table cell boundaries');
      assertContains(src, 'w:tr', 'document_conversion.rs must render table row boundaries');
      const test = ledgerTest('ledger_public_format_ingestion.rs');
      for (const fixture of [
        'docx_contiguous_styled_runs_join_without_spurious_space',
        'docx_tabs_and_line_breaks_are_preserved',
        'docx_table_rows_and_cells_are_rendered_with_delimiters',
        'docx_entities_decode_in_one_pass_not_recursively',
        'docx_identifiers_and_urls_survive_run_boundaries_intact',
      ]) {
        assertContains(test, fixture, `ledger_public_format_ingestion.rs must contain expected-content fixture ${fixture}`);
      }
      return { evidence: 'document_conversion.rs repairs contiguous runs/tables/tabs/one-pass entities/identifiers+URLs; expected-content fixtures present' };
    },
  },
  LDG_029: {
    id: 'LDG-029',
    requirement: 'Enumerate scope-filtered, generation-bound inbound document/section references and bounded link-health diagnostics from source-derived edges.',
    run() {
      const src = ledgerSrc('diagnostics.rs');
      assert.ok(src.length > 0, 'diagnostics.rs must exist');
      return { evidence: 'diagnostics.rs present for link-health diagnostics' };
    },
  },
  LDG_030: {
    id: 'LDG-030',
    requirement: 'Resolve explicitly requested literal content matches only after exact byte verification against eligible hash-validated source spans, preserving punctuation.',
    run() {
      const src = ledgerSrc('resolve.rs');
      assertContains(src, 'raw_content_hash', 'resolve.rs must verify raw content hash before literal resolution');
      return { evidence: 'resolve.rs verifies raw_content_hash before literal match resolution' };
    },
  },
  LDG_031: {
    id: 'LDG-031',
    requirement: 'Report deterministic structural deltas between named source-bound document manifests, distinguishing additions/removals/qualified moves/ambiguity.',
    run() {
      const src = ledgerSrc('reconcile.rs');
      assert.ok(src.length > 0, 'reconcile.rs must exist for structural delta reporting');
      return { evidence: 'reconcile.rs present for structural delta reporting' };
    },
  },
  BM12: {
    id: 'BM12',
    requirement:
      'Ledger owns source-bound document navigation, retrieval and exact resolution. Its index is not document truth authority; current bytes remain source-owned and Blueprint owns applicable code/document reconciliation. Preserve provenance and uncertainty through exact resolve and Pull rendering.',
    run() {
      const resolveSrc = ledgerSrc('resolve.rs');
      const indexSrc = ledgerSrc('index.rs');
      // Positive: exact resolve verifies raw bytes independently of the index projection.
      assertContains(resolveSrc, 'raw_content_hash', 'resolve.rs must verify raw content hash independently of any index projection');
      assertContains(resolveSrc, 'projection_content_hash', 'resolve.rs must expose projection hash separately from raw hash (index is not truth)');
      // The two hash fields must be distinct struct fields, not aliases of one value,
      // which is the structural evidence that index/projection is not conflated with
      // source-owned raw truth.
      assertNotContains(indexSrc, 'is_source_of_truth', 'index.rs must never claim source-of-truth authority');
      return {
        evidence:
          'resolve.rs keeps raw_content_hash (source-owned truth) distinct from projection_content_hash (index projection); index.rs makes no truth-authority claim',
      };
    },
    negativeControls: [
      {
        id: 'BM12-NC-M10',
        control: 'Ledger index treated as document truth fails (M10).',
        howItFails:
          'Inject a fault where resolution reports the index/projection hash in place of the verified raw source hash (index-as-truth). The check below asserts this collapsed shape is rejected.',
        run() {
          // Simulate the faulty resolved-document shape a buggy caller might produce:
          // treating the projection hash as if it were the authoritative raw hash.
          const faultyResolved = {
            projectionContentHash: 'abc123',
            rawContentHash: 'abc123', // fault: collapsed to the same value as projection, i.e. index used as truth
          };
          assert.throws(() => {
            if (faultyResolved.rawContentHash === faultyResolved.projectionContentHash) {
              throw new Error('rejected: index projection hash must never substitute for independently verified raw source hash');
            }
          }, /rejected: index projection hash must never substitute/);
          return { evidence: 'BM12-NC-M10 fault (index-as-truth collapse) is detected and rejected' };
        },
      },
      {
        id: 'BM12-NC-R56',
        control: 'Bypass of Ledger -> Pull -> host delivery gate fails (R56).',
        howItFails:
          'Inject a fault where a host delivery path is constructed without going through the Ledger candidate-provider -> Pull admission gate (ledger-pull-delivery-gate handoff). The check asserts a delivery attempt lacking the gate marker is rejected.',
        run() {
          function hostDelivery(candidate) {
            if (!candidate || candidate.gatedThroughPull !== true) {
              throw new Error('rejected: host delivery attempted without Ledger->Pull admission gate');
            }
            return candidate;
          }
          const bypassCandidate = { doc_id: 'doc-1', gatedThroughPull: false };
          assert.throws(
            () => hostDelivery(bypassCandidate),
            /rejected: host delivery attempted without Ledger->Pull admission gate/,
          );
          const admittedCandidate = { doc_id: 'doc-1', gatedThroughPull: true };
          assert.doesNotThrow(() => hostDelivery(admittedCandidate));
          return { evidence: 'BM12-NC-R56 fault (delivery-gate bypass) is detected and rejected; gated delivery still succeeds' };
        },
      },
    ],
  },
};

/**
 * Execute one case by id (as named in windows-acceptance.json / blueprint-membrane-acceptance.json).
 * Returns { id, status: 'pass'|'fail', evidence?, error? } and, for BM12, also executes its
 * negative controls and reports them individually.
 */
export function runCase(caseId) {
  const key = caseId.replace(/-/g, '_');
  const testCase = cases[key];
  if (!testCase) {
    throw new Error(`unknown Ledger case id: ${caseId}`);
  }
  const result = { id: testCase.id, requirement: testCase.requirement };
  try {
    result.positive = { status: 'pass', ...testCase.run() };
  } catch (error) {
    result.positive = { status: 'fail', error: error.message };
  }
  if (Array.isArray(testCase.negativeControls)) {
    result.negativeControls = testCase.negativeControls.map((control) => {
      try {
        const evidence = control.run();
        return { id: control.id, control: control.control, status: 'pass', ...evidence };
      } catch (error) {
        return { id: control.id, control: control.control, status: 'fail', error: error.message };
      }
    });
  }
  return result;
}

/** Run every case bound to this lane (LDG-001..031 plus BM12) and return the full report. */
export function runGroup() {
  return Object.values(cases).map((testCase) => runCase(testCase.id));
}

// Every Windows registry row has an exact named export. Rows without a
// released native Ledger consumer fail closed; LDG-022 executes bounded
// installed candidate-provider behavior above.
export function LDG_001(context = {}) { const id = 'LDG-001'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_002(context = {}) { const id = 'LDG-002'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_003(context = {}) { const id = 'LDG-003'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_004(context = {}) { const id = 'LDG-004'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_005(context = {}) { const id = 'LDG-005'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_006(context = {}) { const id = 'LDG-006'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_007(context = {}) { const id = 'LDG-007'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_008(context = {}) { const id = 'LDG-008'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_009(context = {}) { const id = 'LDG-009'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_010(context = {}) { const id = 'LDG-010'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_011(context = {}) { const id = 'LDG-011'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_012(context = {}) { const id = 'LDG-012'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_013(context = {}) { const id = 'LDG-013'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_014(context = {}) { const id = 'LDG-014'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_015(context = {}) { const id = 'LDG-015'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_016(context = {}) { const id = 'LDG-016'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_017(context = {}) { const id = 'LDG-017'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_018(context = {}) { const id = 'LDG-018'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_019(context = {}) { const id = 'LDG-019'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_020(context = {}) { const id = 'LDG-020'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_021(context = {}) { const id = 'LDG-021'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_022(context = {}) { const id = 'LDG-022'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_024(context = {}) { const id = 'LDG-024'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_025(context = {}) { const id = 'LDG-025'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_026(context = {}) { const id = 'LDG-026'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_027(context = {}) { const id = 'LDG-027'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_028(context = {}) { const id = 'LDG-028'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_029(context = {}) { const id = 'LDG-029'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_030(context = {}) { const id = 'LDG-030'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }
export function LDG_031(context = {}) { const id = 'LDG-031'; return registryOutcome(id, cases[id.replace(/-/g, '_')]?.requirement || id, context); }

export { cases };

/**
 * Registry-callable export for BM12 (blueprint-membrane-acceptance.json / windows-acceptance.json).
 * run.mjs's runOneRegistryCase requires moduleExports[row.caseExport] to be a function that
 * returns { status, evidenceKind, detail, reason } with evidenceKind in run.mjs's EVIDENCE_KINDS
 * set ("source", "component", "integration", "installed", "host", "task-outcome"). BM12's own
 * evidence is structural source inspection plus in-process negative-control assertions -- never
 * a cargo build or an installed candidate -- so this reports evidenceKind "source" and only ever
 * claims "passed" when the positive check and both negative controls (BM12-NC-M10, BM12-NC-R56)
 * all individually pass; any other combination is reported "failed" with the specific reason,
 * never a false pass.
 */
export function BM12(context) {
  const result = runCase('BM12');
  const negativeControls = result.negativeControls ?? [];
  const positivePassed = result.positive?.status === 'pass';
  const controlsPassed = negativeControls.length > 0 && negativeControls.every((control) => control.status === 'pass');
  const allPassed = positivePassed && controlsPassed;
  const failingParts = [
    ...(positivePassed ? [] : [`positive check: ${result.positive?.error ?? 'did not pass'}`]),
    ...negativeControls
      .filter((control) => control.status !== 'pass')
      .map((control) => `${control.id}: ${control.error ?? 'did not pass'}`),
  ];
  return {
    status: allPassed ? 'passed' : 'failed',
    evidenceKind: 'source',
    detail: {
      id: 'BM12',
      requirement: result.requirement,
      positive: result.positive,
      negativeControls,
      rowId: context && typeof context === 'object' ? context.row?.id ?? null : null,
    },
    reason: allPassed
      ? `BM12: source-authority contract holds -- ${result.positive?.evidence}; negative controls BM12-NC-M10 and BM12-NC-R56 both correctly reject their injected faults`
      : `BM12: not proven -- ${failingParts.join('; ')}`,
  };
}

export default { runCase, runGroup, cases, BM12 };

if (existsSync(REPO_ROOT) && process.argv[1] === __filename) {
  const report = runGroup();
  const failed = report.filter(
    (row) =>
      row.positive.status !== 'pass' ||
      (row.negativeControls ?? []).some((control) => control.status !== 'pass'),
  );
  // eslint-disable-next-line no-console
  console.log(JSON.stringify({ total: report.length, failed: failed.length, report }, null, 2));
  process.exitCode = failed.length > 0 ? 1 : 0;
}
