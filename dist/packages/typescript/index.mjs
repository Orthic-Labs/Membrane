/** Thin transport-injected client for canonical Membrane v1 operation envelopes. */
export const ENVELOPE_VERSION = 1;
export const ERROR_VERSION = 1;
export const CONTEXT_OPERATION = "membrane_context";
export const SOURCE_READ_OPERATION = "membrane_source_read";

export const ERROR_CODES = new Map([
  [CONTEXT_OPERATION, new Set(["context_unavailable", "context_scope_denied", "context_workspace_no_repos", "context_workspace_abstained", "context_deadline_exceeded", "context_caller_scope_binding_denied", "context_caller_not_authorized", "context_cross_root_denied", "context_target_denied", "context_envelope_invalid"])],
  [SOURCE_READ_OPERATION, new Set(["source_read_unavailable", "source_read_hash_mismatch", "source_read_anchor_missing", "source_read_scope_denied", "source_read_envelope_invalid"])],
]);

export class ProtocolError extends Error {
  constructor(code, message, retryable = false, details = {}) {
    super(message);
    this.name = "ProtocolError";
    this.code = code;
    this.retryable = retryable;
    this.details = details;
  }
}

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new ProtocolError("protocol_envelope_invalid", `${label} must be an object`);
  return value;
}

function closed(value, allowed, label) {
  if (Object.keys(value).some((key) => !allowed.has(key))) throw new ProtocolError("protocol_envelope_invalid", `${label} has unknown fields`);
}

export class MembraneClient {
  constructor(transport) {
    if (typeof transport !== "function") throw new TypeError("transport must be callable");
    this.transport = transport;
  }

  context(request) { return this.call(CONTEXT_OPERATION, request); }
  sourceRead(request) { return this.call(SOURCE_READ_OPERATION, request); }

  call(operation, request) {
    if (!ERROR_CODES.has(operation)) throw new ProtocolError("operation_unsupported", `unsupported operation: ${operation}`);
    object(request, "request");
    return validateResponse(operation, this.transport(operation, request));
  }
}

function validateResponse(operation, wire) {
  const response = object(wire, "response");
  closed(response, new Set(["schemaVersion", "operation", "errorVersion", "result"]), "response");
  if (response.schemaVersion !== ENVELOPE_VERSION || response.errorVersion !== ERROR_VERSION) throw new ProtocolError("protocol_version_unsupported", "unsupported operation envelope version");
  if (response.operation !== operation) throw new ProtocolError("protocol_envelope_invalid", "response operation does not match request");
  const result = object(response.result, "result");
  if (result.kind === "success") {
    closed(result, new Set(["kind", "data"]), "success result");
    return object(result.data, "result.data");
  }
  if (result.kind !== "error") throw new ProtocolError("protocol_envelope_invalid", "result kind must be success or error");
  closed(result, new Set(["kind", "code", "message", "retryable", "details"]), "error result");
  if (!ERROR_CODES.get(operation).has(result.code) || typeof result.message !== "string" || !result.message || typeof result.retryable !== "boolean") throw new ProtocolError("protocol_envelope_invalid", "unknown or malformed typed error");
  const details = result.details === undefined ? {} : object(result.details, "error details");
  throw new ProtocolError(result.code, result.message, result.retryable, details);
}
