// D35: portable evidence packs — JSON/Markdown with citations, generation,
// provider tiers, omissions, redaction, and continuation cursors. Bounded,
// cited, and safe to hand off.

import { createHash } from "node:crypto";
import { redactForEgress } from "./redaction.mjs";

export function buildEvidencePack({ repoId, generationId, results, providerTiers, omissions = [], cursor = null, redact = true }) {
  const pack = {
    schemaVersion: 1,
    kind: "evidence-pack",
    repoId,
    generationId,
    providerTiers: providerTiers ?? [],
    results: (results ?? []).map((result) => ({
      id: result.id,
      path: result.path,
      span: result.span ?? null,
      contentHash: result.contentHash ?? null,
      confidenceTier: result.confidenceTier ?? null,
      evidence: (result.evidence ?? []).map((e) => ({ path: e.path, startLine: e.startLine, endLine: e.endLine, contentHash: e.contentHash })),
    })),
    omissions: omissions ?? [],
    continuationCursor: cursor,
    packDigest: null,
  };
  pack.packDigest = createHash("sha256").update(JSON.stringify({ ...pack, packDigest: null })).digest("hex");
  return redact ? redactForEgress(pack) : pack;
}

export function evidencePackToMarkdown(pack) {
  const lines = [
    `# Evidence pack — ${pack.repoId}`,
    "",
    `Generation: \`${pack.generationId}\``,
    `Digest: \`${pack.packDigest}\``,
    "",
    "## Results",
    "",
  ];
  for (const result of pack.results ?? []) {
    const where = result.span ? `${result.path}:${result.span.startLine}-${result.span.endLine}` : result.path;
    lines.push(`- \`${result.id}\` — ${where} (${result.confidenceTier ?? "?"})`);
  }
  if ((pack.omissions ?? []).length) {
    lines.push("", "## Omissions", "");
    for (const omission of pack.omissions) lines.push(`- ${omission.reason ?? omission}`);
  }
  if (pack.continuationCursor) {
    lines.push("", `Continuation cursor: \`${pack.continuationCursor}\``);
  }
  return lines.join("\n");
}
