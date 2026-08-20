import { createHash } from "node:crypto";

// Host capability is an observation of an integration seam. It never grants
// authority, selects evidence, or claims host-internal context enforcement.
export const HOST_EVENTS = Object.freeze([
  "UserPromptSubmit",
  "tool_result_egress",
  "delegated_agent_egress",
  "PreCompact",
  "PostCompact",
  "SessionStart",
  "Stop",
]);

export const HOST_CAPABILITY_MATRIX = Object.freeze({
  schema: "membrane.host-capability-matrix.v1",
  clients: Object.freeze({
    claude_code: Object.freeze({
      UserPromptSubmit: "native",
      tool_result_egress: "projection",
      delegated_agent_egress: "projection",
      PreCompact: "native",
      PostCompact: "native",
      SessionStart: "native",
      Stop: "native",
    }),
    codex: Object.freeze({
      UserPromptSubmit: "native",
      tool_result_egress: "projection",
      delegated_agent_egress: "unavailable",
      PreCompact: "native",
      PostCompact: "native",
      SessionStart: "native",
      Stop: "projection",
    }),
  }),
  levels: Object.freeze(["native", "projection", "unavailable"]),
  semantics: Object.freeze({
    native: "host exposes this lifecycle seam directly",
    projection: "Membrane receives the strongest available parent projection",
    unavailable: "Membrane emits typed degradation and does not claim delivery",
  }),
});

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

export function capabilityFor(client, event) {
  const row = HOST_CAPABILITY_MATRIX.clients[client];
  if (!row || !HOST_EVENTS.includes(event)) return "unavailable";
  return row[event] || "unavailable";
}

export function validateCapabilityMatrix(matrix = HOST_CAPABILITY_MATRIX) {
  const failures = [];
  if (matrix?.schema !== "membrane.host-capability-matrix.v1") failures.push("schema");
  for (const client of ["claude_code", "codex"]) {
    for (const event of HOST_EVENTS) {
      if (!matrix.clients?.[client]?.[event] || !matrix.levels.includes(matrix.clients[client][event])) failures.push(`${client}.${event}`);
    }
  }
  return Object.freeze({ valid: failures.length === 0, failures });
}

export function capabilityMatrixDigest(matrix = HOST_CAPABILITY_MATRIX) {
  return `sha256:${createHash("sha256").update(canonical(matrix)).digest("hex")}`;
}

export const CAPABILITY_MATRIX_DIGEST = capabilityMatrixDigest();
