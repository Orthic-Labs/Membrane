// D30: daemon IPC protocol (S-21). Newline-delimited JSON envelopes with
// requestId, repoId, generation pin, method, deadlineMs, and input; responses
// carry ok/generation/result/error; cancellation targets a requestId.

export const PROTOCOL_VERSION = 1;
export const DEFAULT_DEADLINE_MS = 2000;
export const MIN_DEADLINE_MS = 10;
export const MAX_DEADLINE_MS = 30000;
export const MAX_BUILD_DEADLINE_MS = 120000;

export function validateProtocolVersion(value) {
  if (value !== PROTOCOL_VERSION) {
    throw Object.assign(new Error(`protocolVersion must equal ${PROTOCOL_VERSION}`), { code: "protocol_version_mismatch" });
  }
  return value;
}

export function validateDeadlineMs(value = DEFAULT_DEADLINE_MS, method = null) {
  const maximum = method === "build" ? MAX_BUILD_DEADLINE_MS : MAX_DEADLINE_MS;
  if (!Number.isInteger(value) || value < MIN_DEADLINE_MS || value > maximum) {
    throw Object.assign(new Error(`deadlineMs must be an integer from 10 to ${maximum}`), { code: "deadline_invalid" });
  }
  return value;
}

export function encodeRequest({ requestId, repoId, generation, method, deadlineMs = DEFAULT_DEADLINE_MS, input = {} }) {
  validateDeadlineMs(deadlineMs, method);
  return `${JSON.stringify({ protocolVersion: PROTOCOL_VERSION, requestId, repoId, generation, method, deadlineMs, input })}\n`;
}

export function encodeResponse({ requestId, ok, generation, result, error }) {
  return `${JSON.stringify({ protocolVersion: PROTOCOL_VERSION, requestId, ok, generation, result, error })}\n`;
}

export function encodeCancel(targetRequestId) {
  return `${JSON.stringify({ protocolVersion: PROTOCOL_VERSION, requestId: `cancel-${Date.now()}`, method: "cancel", input: { targetRequestId } })}\n`;
}

export function decodeLine(line) {
  if (!line || !line.trim()) return null;
  try {
    return JSON.parse(line);
  } catch {
    return null;
  }
}

export const METHODS = Object.freeze([
  "status", "search", "resolve", "recall", "expand", "impact", "path", "architecture", "documentTruth", "build",
  // Findings lane (design §7.1 items 5–7) — dispatched to the findings service
  // adapter (src/lib/findings/service.mjs), which owns its freshness model.
  "findings.get", "findings.baseline.capture", "findings.baseline.list", "findings.sarif",
]);
