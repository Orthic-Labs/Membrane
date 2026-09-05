import assert from "node:assert/strict";
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
