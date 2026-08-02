import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const STAGES = ["shadow", "advisory", "context_enforced", "tool_enforced", "learning"];
const REQUIRED = {
  advisory: [{ kind: "source", status: "source_passed" }],
  context_enforced: [{ kind: "mac", status: "mac_host_passed" }, { kind: "windows", status: "windows_host_passed" }],
  tool_enforced: [{ kind: "final", status: "final_passed" }],
  learning: [{ kind: "benchmark", status: "complete", schema: "orthic.e2e-benchmark-result.v1" }],
};
const sha256 = (bytes) => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;

function artifactProof(spec, artifact) {
  if (!artifact?.path || !artifact?.sha256) throw new Error(`rollout promotion missing: ${spec.kind}`);
  const bytes = readFileSync(artifact.path);
  const actual = sha256(bytes);
  if (actual !== artifact.sha256) throw new Error(`rollout artifact digest mismatch: ${spec.kind}`);
  const receipt = JSON.parse(bytes);
  const expectedSchema = spec.schema || "membrane.competitor-parity.v1";
  if (receipt.schema !== expectedSchema || receipt.status !== spec.status || (Array.isArray(receipt.open) && receipt.open.length)) throw new Error(`rollout artifact invalid: ${spec.kind}`);
  return { kind: spec.kind, path: artifact.path, sha256: actual, status: receipt.status };
}

export function advanceRollout(current, next, { artifacts = {} } = {}) {
  const from = STAGES.indexOf(current);
  const to = STAGES.indexOf(next);
  if (from < 0 || to !== from + 1) throw new Error("rollout stage transition is not ordered");
  const evidence = REQUIRED[next].map((spec) => artifactProof(spec, artifacts[spec.kind]));
  return { schema: "orthic.rollout-receipt.v2", from: current, to: next, evidence, status: "promoted" };
}

export { STAGES };
