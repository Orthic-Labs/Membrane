// Membrane protocol — TypeScript/JS binding (dependency-free).
//
// This module is the JS half of the MBR-101 contract. It loads the SAME golden
// fixtures and JSON Schemas the Rust crate embeds, re-implements the SAME
// canonical-byte rules and the SAME minimal JSON-Schema subset, and must
// reproduce the SAME canonical bytes and `sha256:` digests the Rust tests pin.
//
// Canonical-byte contract (mirrors engine/crates/membrane-protocol/src/canonical.rs
// and the pre-existing `canonical()` in mcp/scope-grant-v1.mjs):
//   1. Object keys sorted lexicographically (by UTF-16 code unit, which matches
//      Rust's byte/code-point order for the ASCII keys the contract uses).
//   2. No insignificant whitespace.
//   3. `null` fields preserved; present-but-null != absent.
//   4. Integers bare; the contract's float fields use shortest-round-trip values
//      (e.g. 0.92, 0.5) where JS `JSON.stringify` and Rust `serde_json` agree.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
// bindings/ -> crate -> crates -> engine -> repo root is four levels up.
export const REPO_ROOT = join(HERE, "..", "..", "..", "..");

/** Canonical JSON serialization (sorted keys, no whitespace). */
export function canonicalize(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalize).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${canonicalize(value[k])}`).join(",")}}`;
}
/** `sha256:<hex>` of an arbitrary string's UTF-8 bytes. */
export function digestStr(text) {
  return `sha256:${createHash("sha256").update(text, "utf8").digest("hex")}`;
}

/** Canonical digest of a parsed JSON value. */
export function canonicalDigest(value) {
  return digestStr(canonicalize(value));
}

/** The five typed contract shapes and their on-disk artifacts. */
export const SHAPES = [
  { name: "ScopeGrantV1", schema: "schemas/scope-grant.v1.schema.json", fixture: "operations/scope-grant.v1.golden.json" },
  { name: "ContextCandidateSetV1", schema: "schemas/context-candidate-set.v1.schema.json", fixture: "operations/context-candidate-set.v1.golden.json" },
  { name: "ContextPacketV1", schema: "schemas/context-packet.v1.schema.json", fixture: "operations/context-packet.v1.golden.json" },
  { name: "ContextReceiptV1", schema: "schemas/context-receipt.v1.schema.json", fixture: "operations/context-receipt.v1.golden.json" },
  { name: "KnowledgeEmissionV1", schema: "schemas/knowledge-emission.v1.schema.json", fixture: "operations/knowledge-emission.v1.golden.json" },
];

/** Read + parse a repo-relative JSON file. */
export function loadJson(repoRelativePath) {
  return JSON.parse(readFileSync(join(REPO_ROOT, repoRelativePath), "utf8"));
}

// --- Minimal JSON-Schema validator (same subset as tests/schema.rs) ---------

function typeMatches(expected, instance) {
  switch (expected) {
    case "object": return instance !== null && typeof instance === "object" && !Array.isArray(instance);
    case "array": return Array.isArray(instance);
    case "string": return typeof instance === "string";
    case "boolean": return typeof instance === "boolean";
    case "null": return instance === null;
    case "integer": return typeof instance === "number" && Number.isInteger(instance);
    case "number": return typeof instance === "number";
    default: return false;
  }
}

function isLowerHex(ch) {
  return (ch >= "0" && ch <= "9") || (ch >= "a" && ch <= "f");
}

function patternMatches(pattern, text) {
  if (pattern === "^sha256:[a-f0-9]{64}$") {
    const rest = text.startsWith("sha256:") ? text.slice(7) : null;
    return rest !== null && rest.length === 64 && [...rest].every(isLowerHex);
  }
  if (pattern === "^scope-grant-ed25519-v1:[a-f0-9]{32}$") {
    const prefix = "scope-grant-ed25519-v1:";
    const rest = text.startsWith(prefix) ? text.slice(prefix.length) : null;
    return rest !== null && rest.length === 32 && [...rest].every(isLowerHex);
  }
  if (pattern === "^sgv1-") return text.startsWith("sgv1-");
  return text.includes(pattern.replace(/^\^/, "").replace(/\$$/, ""));
}

function isDateTime(text) {
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/.test(text);
}

function resolveRef(root, schema) {
  const ref = schema && schema.$ref;
  if (typeof ref === "string" && ref.startsWith("#/")) {
    let node = root;
    for (const segment of ref.slice(2).split("/")) node = node[segment];
    return node;
  }
  return schema;
}

function validateInto(schema, instance, root, path, errors) {
  schema = resolveRef(root, schema);
  if (schema.const !== undefined && JSON.stringify(schema.const) !== JSON.stringify(instance)) {
    errors.push(`${path}: const mismatch, expected ${JSON.stringify(schema.const)}`);
  }
  if (Array.isArray(schema.enum) && !schema.enum.some((v) => JSON.stringify(v) === JSON.stringify(instance))) {
    errors.push(`${path}: not in enum ${JSON.stringify(schema.enum)}`);
  }
  if (schema.type !== undefined) {
    const types = Array.isArray(schema.type) ? schema.type : [schema.type];
    if (!types.some((t) => typeMatches(t, instance))) {
      errors.push(`${path}: type mismatch, expected one of ${JSON.stringify(types)}`);
      return;
    }
  }

  if (instance !== null && typeof instance === "object" && !Array.isArray(instance)) {
    for (const key of schema.required ?? []) {
      if (!(key in instance)) errors.push(`${path}: missing required key \`${key}\``);
    }
    const properties = schema.properties ?? {};
    for (const [key, subschema] of Object.entries(properties)) {
      if (key in instance) validateInto(subschema, instance[key], root, `${path}.${key}`, errors);
    }
    if (schema.additionalProperties === false) {
      for (const key of Object.keys(instance)) {
        if (!(key in properties)) errors.push(`${path}: unexpected additional key \`${key}\``);
      }
    } else if (schema.additionalProperties && typeof schema.additionalProperties === "object") {
      for (const key of Object.keys(instance)) {
        if (!(key in properties)) validateInto(schema.additionalProperties, instance[key], root, `${path}.${key}`, errors);
      }
    }
  } else if (Array.isArray(instance)) {
    if (schema.items) instance.forEach((item, i) => validateInto(schema.items, item, root, `${path}[${i}]`, errors));
  } else if (typeof instance === "string") {
    if (schema.minLength !== undefined && [...instance].length < schema.minLength) {
      errors.push(`${path}: shorter than minLength ${schema.minLength}`);
    }
    if (schema.pattern !== undefined && !patternMatches(schema.pattern, instance)) {
      errors.push(`${path}: does not match pattern ${schema.pattern}`);
    }
    if (schema.format === "date-time" && !isDateTime(instance)) {
      errors.push(`${path}: not an RFC 3339 date-time`);
    }
  } else if (typeof instance === "number") {
    if (schema.minimum !== undefined && instance < schema.minimum) errors.push(`${path}: below minimum ${schema.minimum}`);
    if (schema.maximum !== undefined && instance > schema.maximum) errors.push(`${path}: above maximum ${schema.maximum}`);
  }
}

/** Validate `instance` against `schema`; returns an array of violations. */
export function validate(schema, instance) {
  const errors = [];
  validateInto(schema, instance, schema, "$", errors);
  return errors;
}
