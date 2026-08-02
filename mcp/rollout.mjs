import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DEFAULT_MEMBRANE_ROOT = resolve(HERE, "..");
const DEFAULT_WORKSPACE_ROOT = resolve(DEFAULT_MEMBRANE_ROOT, "..");

const STAGES = ["shadow", "advisory", "context_enforced", "tool_enforced", "learning"];
const REQUIRED = {
  advisory: [{ kind: "source", status: "source_passed" }],
  context_enforced: [{ kind: "mac", status: "mac_host_passed" }, { kind: "windows", status: "windows_host_passed" }],
  tool_enforced: [{ kind: "final", status: "final_passed" }],
  learning: [{ kind: "benchmark", status: "complete", schema: "orthic.e2e-benchmark-result.v1" }],
};
const sha256 = (bytes) => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;

/** Live current commit for a repo root. Never cached, never caller-supplied — this is what
 * makes a qualification artifact's commit binding meaningful instead of self-asserted. */
export function commit(cwd) {
  try { return execFileSync("git", ["rev-parse", "HEAD"], { cwd, encoding: "utf8" }).trim(); }
  catch { return null; }
}

/** Which commit a given receipt schema claims to have qualified against. */
function receiptCommit(receipt) {
  if (receipt.schema === "orthic.e2e-benchmark-result.v1") return receipt.manifest?.workspace_commit || null;
  return receipt.fingerprint?.membrane || null;
}

function artifactProof(spec, artifact, live) {
  if (!artifact?.path || !artifact?.sha256) throw new Error(`rollout promotion missing: ${spec.kind}`);
  const bytes = readFileSync(artifact.path);
  const actual = sha256(bytes);
  if (actual !== artifact.sha256) throw new Error(`rollout artifact digest mismatch: ${spec.kind}`);
  const receipt = JSON.parse(bytes);
  const expectedSchema = spec.schema || "membrane.competitor-parity.v1";
  if (receipt.schema !== expectedSchema || receipt.status !== spec.status || (Array.isArray(receipt.open) && receipt.open.length)) throw new Error(`rollout artifact invalid: ${spec.kind}`);
  // Hash-binding alone only proves the file was not altered after being written — it does not
  // prove the qualification it records still describes the code at HEAD. Require every artifact
  // to carry the commit it was produced against, and require that commit to be the one actually
  // checked out right now. A receipt from three commits ago can no longer certify a promotion.
  const recorded = receiptCommit(receipt);
  if (!recorded || recorded === "unknown") throw new Error(`rollout artifact missing commit binding: ${spec.kind}`);
  const expectedCommit = receipt.schema === "orthic.e2e-benchmark-result.v1" ? live.workspaceCommit : live.membraneCommit;
  if (expectedCommit && recorded !== expectedCommit) throw new Error(`rollout artifact stale: ${spec.kind} (qualified ${recorded}, HEAD is ${expectedCommit})`);
  return { kind: spec.kind, path: artifact.path, sha256: actual, status: receipt.status, commit: recorded };
}

export function advanceRollout(current, next, { artifacts = {}, membraneRoot = DEFAULT_MEMBRANE_ROOT, workspaceRoot = DEFAULT_WORKSPACE_ROOT } = {}) {
  const from = STAGES.indexOf(current);
  const to = STAGES.indexOf(next);
  if (from < 0 || to !== from + 1) throw new Error("rollout stage transition is not ordered");
  const live = { membraneCommit: commit(membraneRoot), workspaceCommit: commit(workspaceRoot) };
  const evidence = REQUIRED[next].map((spec) => artifactProof(spec, artifacts[spec.kind], live));
  return { schema: "orthic.rollout-receipt.v2", from: current, to: next, evidence, status: "promoted" };
}

export { STAGES };
