import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const CLIENT = join(dirname(fileURLToPath(import.meta.url)), "client.mjs");
const LEVELS = { cursor: "L1", windsurf: "L1", generic_mcp: "L0" };

export function adapterManifest(name) {
  if (!LEVELS[name]) throw new Error("unsupported_adapter");
  return { schema: "orthic.adapter-shim.v1", adapter_id: name, max_honest_level: LEVELS[name], injection: "membrane_context", tool_receipts: false, response_gate: false };
}

export function requestContext(name, request, { root = process.cwd(), client = CLIENT } = {}) {
  const manifest = adapterManifest(name);
  const result = spawnSync(process.execPath, [client, "--input", "-"], { cwd: root, input: `${JSON.stringify(request)}\n`, encoding: "utf8", timeout: 1500, windowsHide: true });
  if (result.error || result.status !== 0) return { ...manifest, state: "degraded", reason: result.error?.code === "ETIMEDOUT" ? "context_timeout" : "context_unavailable" };
  try {
    const payload = JSON.parse(result.stdout.trim());
    return { ...manifest, state: payload.ok && payload.packet ? "context_enforced" : "advisory", packet: payload.packet || null, omissions: payload.degradationReason && payload.degradationReason !== "none" ? [payload.degradationReason] : [] };
  } catch {
    return { ...manifest, state: "degraded", reason: "malformed_context_response" };
  }
}
