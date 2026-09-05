from pathlib import Path
import subprocess

ROOT = Path.cwd()


def replace_once(path, old, new):
    p = ROOT / path
    text = p.read_text()
    if text.count(old) != 1:
        raise SystemExit(f"expected exactly one match in {path}: {old[:80]!r}; got {text.count(old)}")
    p.write_text(text.replace(old, new, 1))

# BPT-051/BPT-052: expose source-bound finding explanation and governed evidence packs
# through the existing resident findings service. No MCP tool is added.
service = "blueprint/src/lib/findings/service.mjs"
replace_once(
    service,
    'import { fail } from "../application/errors.mjs";\nimport { toSarif } from "../sarif.mjs";\n',
    'import { fail } from "../application/errors.mjs";\nimport { buildEvidencePack } from "../evidence-pack.mjs";\nimport { toSarif } from "../sarif.mjs";\n',
)
replace_once(
    service,
    '  "findings.get",\n  "findings.baseline.capture",\n',
    '  "findings.get",\n  "findings.explain",\n  "findings.evidence_pack",\n  "findings.baseline.capture",\n',
)
replace_once(
    service,
    '      generationId,\n      manifestDigest: meta.manifest.manifestDigest ?? null,\n      baseCommit: meta.manifest.repo?.baseCommit ?? null,\n',
    '      generationId,\n      repoId: meta.manifest.repo?.repoId ?? null,\n      manifestDigest: meta.manifest.manifestDigest ?? null,\n      baseCommit: meta.manifest.repo?.baseCommit ?? null,\n',
)
anchor = '''  /** findings.baseline.capture {name} — persist the current bundle as a named\n   * generation baseline beside existing daemon state (~/.blueprint). */\n'''
insert = '''  function findingEvidenceRows(finding, bundle) {\n    return (finding.evidencePath ?? []).map((path, index) => ({\n      path,\n      startLine: index === 0 ? finding.startLine ?? null : null,\n      endLine: index === 0 ? finding.endLine ?? finding.startLine ?? null : null,\n      contentHash: finding.perFileContentHashes?.[path] ?? bundle.perFileContentHashes?.[path] ?? null,\n    }));\n  }\n\n  async function governedFindingContext(input = {}, { signal } = {}) {\n    const { root, effectiveOutDir, sealed } = await currentGeneration(input, { signal });\n    const overlay = await Promise.resolve(freshnessOverlay(root, sealed.baseCommit));\n    const stale = !isCurrentOverlay(overlay);\n    enforceFreshnessTolerance(stale, overlay, sealed, input);\n    const bundle = await bundleFor(root, effectiveOutDir, sealed, { signal });\n    throwIfAborted(signal);\n    return { root, sealed, overlay, stale, bundle };\n  }\n\n  /** findings.explain {fingerprint, generation?, allowStale?}\n   * Explain one exact finding from the same generation-bound bundle that\n   * findings.get serves. The result contains rule reasoning and source/hash\n   * evidence only; it never invents remediation evidence or executes code. */\n  async function findingsExplain(input = {}, options = {}) {\n    const fingerprint = String(input.fingerprint ?? "").trim();\n    if (!fingerprint) fail("finding_id_invalid", "findings.explain requires a finding fingerprint.");\n    const { root, sealed, overlay, stale, bundle } = await governedFindingContext(input, options);\n    const finding = bundle.findings.find((entry) => entry.fingerprint === fingerprint);\n    if (!finding) fail("finding_not_found", `Finding ${fingerprint} is not present in generation ${sealed.generationId}.`, { fingerprint, generationId: sealed.generationId });\n    return {\n      schemaVersion: 1,\n      kind: "findings.explain",\n      root,\n      generationId: sealed.generationId,\n      freshness: stale ? "stale" : "current",\n      finding: {\n        fingerprint: finding.fingerprint,\n        ruleId: finding.ruleId,\n        severity: finding.severity,\n        path: finding.path,\n        startLine: finding.startLine ?? null,\n        endLine: finding.endLine ?? finding.startLine ?? null,\n        name: finding.name ?? null,\n        specifier: finding.specifier ?? null,\n      },\n      reasoning: {\n        ruleName: finding.ruleName,\n        description: finding.ruleDescription,\n        message: finding.message,\n        remediation: finding.remediation,\n        precisionTier: finding.precisionTier,\n        confidenceTier: finding.confidenceTier,\n      },\n      evidence: findingEvidenceRows(finding, bundle),\n      omissions: stale ? stalenessOmissions(overlay) : [],\n    };\n  }\n\n  /** findings.evidence_pack {fingerprints:[...], generation?, allowStale?}\n   * Produce a portable, redacted pack only for explicitly selected findings.\n   * The resident service owns generation/freshness admission; buildEvidencePack\n   * is only the deterministic rendering step. */\n  async function findingsEvidencePack(input = {}, options = {}) {\n    const fingerprints = [...new Set((Array.isArray(input.fingerprints) ? input.fingerprints : []).map((value) => String(value).trim()).filter(Boolean))];\n    if (!fingerprints.length) fail("finding_selection_empty", "findings.evidence_pack requires at least one finding fingerprint.");\n    if (fingerprints.length > 100) fail("finding_selection_too_large", "findings.evidence_pack accepts at most 100 finding fingerprints.");\n    const { root, sealed, overlay, stale, bundle } = await governedFindingContext(input, options);\n    const byFingerprint = new Map(bundle.findings.map((finding) => [finding.fingerprint, finding]));\n    const missing = fingerprints.filter((fingerprint) => !byFingerprint.has(fingerprint));\n    if (missing.length) fail("finding_not_found", "One or more selected findings are not present in the served generation.", { fingerprints: missing, generationId: sealed.generationId });\n    const selected = fingerprints.map((fingerprint) => byFingerprint.get(fingerprint));\n    const pack = buildEvidencePack({\n      repoId: sealed.repoId ?? repositoryStateKey(root),\n      generationId: sealed.generationId,\n      providerTiers: [...new Set(selected.map((finding) => finding.precisionTier).filter(Boolean))].sort(),\n      results: selected.map((finding) => ({\n        id: finding.fingerprint,\n        path: finding.path,\n        span: { startLine: finding.startLine ?? 1, endLine: finding.endLine ?? finding.startLine ?? 1 },\n        contentHash: finding.perFileContentHashes?.[finding.path] ?? bundle.perFileContentHashes?.[finding.path] ?? null,\n        confidenceTier: finding.confidenceTier ?? null,\n        evidence: findingEvidenceRows(finding, bundle),\n      })),\n      omissions: [\n        ...(stale ? stalenessOmissions(overlay).map((entry) => ({ reason: entry.code, detail: entry.detail })) : []),\n        ...(bundle.omissions.length ? [{ reason: "source_coverage_omissions", count: bundle.omissions.length }] : []),\n      ],\n      redact: true,\n    });\n    return { schemaVersion: 1, kind: "findings.evidence_pack", root, generationId: sealed.generationId, freshness: stale ? "stale" : "current", pack };\n  }\n\n'''
replace_once(service, anchor, insert + anchor)
replace_once(
    service,
    '    "findings.get": findingsGet,\n    "findings.baseline.capture": baselineCapture,\n',
    '    "findings.get": findingsGet,\n    "findings.explain": findingsExplain,\n    "findings.evidence_pack": findingsEvidencePack,\n    "findings.baseline.capture": baselineCapture,\n',
)

replace_once(
    "blueprint/src/service/protocol.mjs",
    '  "findings.get", "findings.baseline.capture", "findings.baseline.list", "findings.sarif",\n',
    '  "findings.get", "findings.explain", "findings.evidence_pack", "findings.baseline.capture", "findings.baseline.list", "findings.sarif",\n',
)

replace_once(
    "blueprint/src/service/client.mjs",
    '''  findingsSarif(input = {}, options = {}) {\n    return this.request({ method: "findings.sarif", input, deadlineMs: MAX_DEADLINE_MS, ...options });\n  }\n\n''',
    '''  findingsSarif(input = {}, options = {}) {\n    return this.request({ method: "findings.sarif", input, deadlineMs: MAX_DEADLINE_MS, ...options });\n  }\n\n  findingsExplain(input = {}, options = {}) {\n    return this.request({ method: "findings.explain", input, deadlineMs: MAX_DEADLINE_MS, ...options });\n  }\n\n  findingsEvidencePack(input = {}, options = {}) {\n    return this.request({ method: "findings.evidence_pack", input, deadlineMs: MAX_DEADLINE_MS, ...options });\n  }\n\n''',
)

replace_once(
    "blueprint/tests/findings-service.test.mjs",
    '''  assert.deepEqual([...FINDINGS_SERVICE_METHODS].sort(), [\n    "findings.baseline.capture",\n    "findings.baseline.list",\n    "findings.get",\n    "findings.sarif",\n  ]);\n''',
    '''  assert.deepEqual([...FINDINGS_SERVICE_METHODS].sort(), [\n    "findings.baseline.capture",\n    "findings.baseline.list",\n    "findings.evidence_pack",\n    "findings.explain",\n    "findings.get",\n    "findings.sarif",\n  ]);\n''',
)

new_test = ROOT / "blueprint/tests/findings-explain-pack.test.mjs"
new_test.write_text(r'''import assert from "node:assert/strict";
import test from "node:test";

import { createFindingsService } from "../src/lib/findings/service.mjs";

const FILES = {
  "src/target.ts": "export const present = 1;\n",
  "src/user.ts": "import { missing } from './target.js';\nexport const value = missing;\n",
};

function service() {
  return createFindingsService({
    sealedGeneration: () => ({ generationId: "gen-findings", repoId: "repo-findings", manifestDigest: "sha256:m", baseCommit: null }),
    freshnessOverlay: () => ({ available: true, stable: true, limitExceeded: false, entries: [], reason: null }),
    scanRepository: () => Object.entries(FILES).map(([path, text]) => ({ path, text })),
  });
}

test("findings.explain binds rule reasoning to source/hash evidence", async () => {
  const api = service();
  const listed = await api["findings.get"]({ repoRoot: "/repo" });
  assert.equal(listed.findings.length, 1);
  const finding = listed.findings[0];
  const explained = await api["findings.explain"]({ repoRoot: "/repo", fingerprint: finding.fingerprint });
  assert.equal(explained.kind, "findings.explain");
  assert.equal(explained.generationId, "gen-findings");
  assert.equal(explained.finding.fingerprint, finding.fingerprint);
  assert.equal(explained.reasoning.ruleName, "import-binding-not-exported");
  assert.ok(explained.reasoning.description);
  assert.ok(explained.reasoning.message.includes("missing"));
  assert.deepEqual(explained.evidence.map((entry) => entry.path), ["src/user.ts", "src/target.ts"]);
  assert.ok(explained.evidence.every((entry) => typeof entry.contentHash === "string" && entry.contentHash.startsWith("sha256:")));
});

test("findings.explain fails closed for a finding outside the served generation", async () => {
  await assert.rejects(service()["findings.explain"]({ repoRoot: "/repo", fingerprint: "not-here" }), { code: "finding_not_found" });
});

test("findings.evidence_pack includes only selected findings and is generation-bound", async () => {
  const api = service();
  const listed = await api["findings.get"]({ repoRoot: "/repo" });
  const fingerprint = listed.findings[0].fingerprint;
  const result = await api["findings.evidence_pack"]({ repoRoot: "/repo", fingerprints: [fingerprint] });
  assert.equal(result.kind, "findings.evidence_pack");
  assert.equal(result.generationId, "gen-findings");
  assert.equal(result.pack.repoId, "repo-findings");
  assert.equal(result.pack.generationId, "gen-findings");
  assert.deepEqual(result.pack.results.map((entry) => entry.id), [fingerprint]);
  assert.ok(result.pack.results[0].evidence.every((entry) => entry.contentHash));
  assert.match(result.pack.packDigest, /^[0-9a-f]{64}$/);
});

test("findings.evidence_pack requires an explicit bounded selection", async () => {
  const api = service();
  await assert.rejects(api["findings.evidence_pack"]({ repoRoot: "/repo", fingerprints: [] }), { code: "finding_selection_empty" });
  await assert.rejects(api["findings.evidence_pack"]({ repoRoot: "/repo", fingerprints: Array.from({ length: 101 }, (_, i) => `f${i}`) }), { code: "finding_selection_too_large" });
});
''')

# Run focused no-build regressions before the feature commit.
subprocess.run([
    "node", "--test",
    "blueprint/tests/findings-service.test.mjs",
    "blueprint/tests/findings-explain-pack.test.mjs",
    "blueprint/tests/evidence-pack.test.mjs",
    "blueprint/tests/ipc-protocol.test.mjs",
], check=True)

subprocess.run(["git", "config", "user.name", "Blueprint completion automation"], check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], check=True)
subprocess.run(["git", "add", service, "blueprint/src/service/protocol.mjs", "blueprint/src/service/client.mjs", "blueprint/tests/findings-service.test.mjs", str(new_test.relative_to(ROOT))], check=True)
subprocess.run(["git", "commit", "-m", "feat(blueprint-findings): explain findings and serve governed evidence packs", "-m", "Close BPT-051 and BPT-052 at the resident service boundary. Explain one exact generation-bound finding with rule reasoning and source/hash evidence, and render explicitly selected findings as redacted portable evidence packs. Keep the six MCP tools unchanged; daemon IPC remains the governed host path."], check=True)

# Consume the one-use transport and restore the branch check lane to read-only.
workflow = ROOT / ".github/workflows/blueprint-completion.yml"
w = workflow.read_text()
w = w.replace('    permissions:\n      contents: write\n', '')
w = w.replace('      - name: Apply reviewed baseline closure\n        run: python3 .github/blueprint-completion-input/apply.py\n', '')
workflow.write_text(w)
subprocess.run(["git", "rm", "-r", ".github/blueprint-completion-input"], check=True)
subprocess.run(["git", "add", str(workflow.relative_to(ROOT))], check=True)
subprocess.run(["git", "commit", "-m", "ci(blueprint): remove findings completion transport"], check=True)
subprocess.run(["git", "push", "origin", "HEAD:blueprint-completion"], check=True)
