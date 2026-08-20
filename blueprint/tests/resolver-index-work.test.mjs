import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { buildGraphGeneration, scanSourcesPublic } from "../src/graph/static-provider.mjs";
import { containsCall } from "../src/graph/language-extractors.mjs";

function legacyCallEdges(files, nodes, edges) {
  const targets = nodes.filter((node) => node.kind === "symbol" && ["Method", "Function"].some((label) => node.labels.includes(label)));
  const sources = nodes.filter((node) => node.kind === "symbol" && ["Method", "Function", "Test"].some((label) => node.labels.includes(label)));
  const targetsByName = new Map();
  for (const target of targets) {
    const name = target.qualifiedName.split(".").at(-1);
    const matches = targetsByName.get(name) ?? [];
    matches.push(target);
    targetsByName.set(name, matches);
  }
  const importsByPath = new Map();
  for (const imported of edges.filter((edge) => edge.kind === "IMPORTS" && edge.target !== null)) {
    const sourcePath = imported.source.replace(/^file:/, "");
    const targetPath = imported.target.replace(/^file:/, "");
    const targetsForSource = importsByPath.get(sourcePath) ?? new Set();
    targetsForSource.add(targetPath);
    importsByPath.set(sourcePath, targetsForSource);
  }
  const result = [];
  for (const source of sources) {
    const file = files.find((item) => item.path === source.path);
    if (!file) continue;
    const evidence = source.evidence[0];
    const body = file.lines.slice(evidence.startLine - 1, evidence.endLine).join("\n");
    for (const [name, candidates] of targetsByName) {
      if (!containsCall(file, body, name)) continue;
      const sameFile = candidates.filter((target) => target.path === source.path && target.id !== source.id);
      const imported = candidates.filter((target) => (importsByPath.get(source.path) ?? new Set()).has(target.path) && target.id !== source.id);
      const fallback = candidates.length === 1 && candidates[0].id !== source.id ? candidates : [];
      const resolved = sameFile.length > 0 ? sameFile : imported.length > 0 ? imported : fallback;
      for (const target of resolved) result.push(`CALLS:${source.id}:${target.id}`);
      const other = candidates.filter((target) => target.id !== source.id);
      if (resolved.length === 0 && other.length > 1) result.push(`CALLS:${source.id}:unresolved:${name}`);
    }
  }
  return result;
}

test("resolver index preserves legacy call output with materially fewer candidate inspections", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-resolver-index-"));
  try {
    execFileSync("git", ["init", "-q"], { cwd: root });
    const targets = Array.from({ length: 96 }, (_, index) => `export function target${String(index).padStart(3, "0")}() { return ${index}; }`);
    writeFileSync(join(root, "targets.ts"), `${targets.join("\n")}\n`);
    writeFileSync(join(root, "caller.ts"), "export function caller() { return target000(); }\n");
    execFileSync("git", ["add", "-A"], { cwd: root });
    const work = {};
    const generation = buildGraphGeneration(root, { resolverWork: work });
    const indexed = generation.edges
      .filter((edge) => edge.kind === "CALLS")
      .map((edge) => `${edge.kind}:${edge.source}:${edge.target ?? `unresolved:${edge.specifier}`}`);
    assert.deepEqual(indexed, legacyCallEdges(scanSourcesPublic(root).files, generation.nodes, generation.edges),
      "indexed resolution must retain legacy ordered call output");
    assert.ok(work.candidateInspections < work.fullCandidateInspections / 10,
      `expected indexed candidate work (${work.candidateInspections}) to be <10% of legacy scan (${work.fullCandidateInspections})`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
