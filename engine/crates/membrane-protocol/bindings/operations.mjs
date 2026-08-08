// Operations contract — TypeScript/JS half of the MBR-301 acceptance.
//
// This module is the JS counterpart to `engine/crates/membrane-protocol/src/operations.rs`.
// It loads the SAME on-disk JSON Schemas and golden fixtures the Rust side
// uses, re-implements the SAME canonical-byte rules and the SAME minimal
// JSON-Schema subset, and must reproduce the SAME per-operation
// validation results and the SAME operations-index canonical digest the
// Rust side pins.
//
// Independent-versioning rule (mirrored on both sides):
//   * Each operation has its own `schemaVersion` and `errorVersion`.
//   * Bumping one operation's version NEVER moves a sibling.
//   * The cross-operation registry (`OPERATIONS`) is the only place that
//     observes the whole set; everything else is per-operation.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { canonicalize, canonicalDigest, loadJson, validate } from "./protocol.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
// bindings/ -> crate -> crates -> engine -> repo root is four levels up.
export const REPO_ROOT = join(HERE, "..", "..", "..", "..");

/** The cross-operation registry. Must match the Rust `OPERATIONS` constant
 *  AND the on-disk `operations/operations/operations-index.v1.golden.json`
 *  fixture in name, schemaVersion, errorVersion, and error-code set. */
export const OPERATIONS = [
  {
    name: "membrane_context",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-context.v1.schema.json",
    successFixture: "operations/operations/membrane-context.v1.golden.json",
    errorFixture: "operations/operations/membrane-context.v1.error.golden.json",
    errorCodes: [
      "context_unavailable",
      "context_scope_denied",
      "context_workspace_no_repos",
      "context_workspace_abstained",
      "context_deadline_exceeded",
      "context_caller_scope_binding_denied",
      "context_caller_not_authorized",
      "context_cross_root_denied",
      "context_target_denied",
      "context_envelope_invalid",
    ],
  },
  {
    name: "membrane_source_read",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-source-read.v1.schema.json",
    successFixture: "operations/operations/membrane-source-read.v1.golden.json",
    errorFixture: "operations/operations/membrane-source-read.v1.error.golden.json",
    errorCodes: [
      "source_read_unavailable",
      "source_read_hash_mismatch",
      "source_read_anchor_missing",
      "source_read_scope_denied",
      "source_read_envelope_invalid",
    ],
  },
  {
    name: "membrane_knowledge_propose",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-knowledge-propose.v1.schema.json",
    successFixture: "operations/operations/membrane-knowledge-propose.v1.golden.json",
    errorFixture: "operations/operations/membrane-knowledge-propose.v1.error.golden.json",
    errorCodes: [
      "proposal_emission_text_required",
      "proposal_payload_too_large",
      "proposal_rate_limited",
      "proposal_binding_unresolvable",
      "proposal_durable_write_failed",
      "proposal_scope_denied",
    ],
  },
  {
    name: "membrane_checkpoint_save",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-checkpoint-save.v1.schema.json",
    successFixture: "operations/operations/membrane-checkpoint-save.v1.golden.json",
    errorFixture: "operations/operations/membrane-checkpoint-save.v1.error.golden.json",
    errorCodes: [
      "checkpoint_payload_too_large",
      "checkpoint_rate_limited",
      "checkpoint_scope_denied",
      "checkpoint_save_unavailable",
      "checkpoint_envelope_invalid",
    ],
  },
  {
    name: "membrane_checkpoint_load",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-checkpoint-load.v1.schema.json",
    successFixture: "operations/operations/membrane-checkpoint-load.v1.golden.json",
    errorFixture: "operations/operations/membrane-checkpoint-load.v1.error.golden.json",
    errorCodes: [
      "checkpoint_not_found",
      "checkpoint_expired",
      "checkpoint_scope_denied",
      "checkpoint_load_unavailable",
      "checkpoint_envelope_invalid",
    ],
  },
  {
    name: "membrane_working_context",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-working-context.v1.schema.json",
    successFixture: "operations/operations/membrane-working-context.v1.golden.json",
    errorFixture: "operations/operations/membrane-working-context.v1.error.golden.json",
    errorCodes: [
      "working_context_payload_too_large",
      "working_context_rate_limited",
      "working_context_scope_required",
      "working_context_id_required",
      "working_context_operation_invalid",
      "working_context_scope_denied",
      "working_context_envelope_invalid",
    ],
  },
  {
    name: "membrane_temporal_fact",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-temporal-fact.v1.schema.json",
    successFixture: "operations/operations/membrane-temporal-fact.v1.golden.json",
    errorFixture: "operations/operations/membrane-temporal-fact.v1.error.golden.json",
    errorCodes: [
      "temporal_fact_payload_too_large",
      "temporal_fact_scope_denied",
      "temporal_fact_scope_mismatch",
      "temporal_fact_query_invalid",
      "temporal_fact_operation_invalid",
      "temporal_fact_envelope_invalid",
    ],
  },
  {
    name: "membrane_scratchpad",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-scratchpad.v1.schema.json",
    successFixture: "operations/operations/membrane-scratchpad.v1.golden.json",
    errorFixture: "operations/operations/membrane-scratchpad.v1.error.golden.json",
    errorCodes: [
      "scratchpad_payload_too_large",
      "scratchpad_scope_required",
      "scratchpad_scope_denied",
      "scratchpad_operation_invalid",
      "scratchpad_envelope_invalid",
    ],
  },
  {
    name: "membrane_feedback",
    schemaVersion: 1,
    errorVersion: 1,
    schemaPath: "schemas/operations/membrane-feedback.v1.schema.json",
    successFixture: "operations/operations/membrane-feedback.v1.golden.json",
    errorFixture: "operations/operations/membrane-feedback.v1.error.golden.json",
    errorCodes: [
      "feedback_invalid",
      "feedback_invalid_verdict_ref",
      "feedback_payload_too_large",
      "feedback_rate_limited",
      "feedback_binding_unresolvable",
      "feedback_durable_write_failed",
      "feedback_independent_readback_mismatch",
    ],
  },
  { name: "hub.capabilities", schemaVersion: 1, errorVersion: 1, schemaPath: "schemas/operations/hub-capabilities.v1.schema.json", successFixture: "operations/operations/hub-capabilities.v1.golden.json", errorFixture: "operations/operations/hub-capabilities.v1.error.golden.json", errorCodes: ["hub_unavailable"] },
  { name: "hub.snapshot", schemaVersion: 1, errorVersion: 1, schemaPath: "schemas/operations/hub-snapshot.v1.schema.json", successFixture: "operations/operations/hub-snapshot.v1.golden.json", errorFixture: "operations/operations/hub-snapshot.v1.error.golden.json", errorCodes: ["hub_unavailable"] },
];

/** Validate one operation's success and error fixtures against the
 *  per-operation JSON Schema. Throws with a descriptive message on the
 *  first violation. Mirrors the Rust
 *  `every_operation_fixtures_validate` test. */
export function validateOperationFixtures(operation) {
  const schema = loadJson(operation.schemaPath);
  const success = loadJson(operation.successFixture);
  const error = loadJson(operation.errorFixture);
  const successViolations = validate(schema, success);
  if (successViolations.length > 0) {
    throw new Error(
      `${operation.name}: success fixture fails schema validation: ${JSON.stringify(successViolations)}`,
    );
  }
  const errorViolations = validate(schema, error);
  if (errorViolations.length > 0) {
    throw new Error(
      `${operation.name}: error fixture fails schema validation: ${JSON.stringify(errorViolations)}`,
    );
  }
  // Every closed error-code in the operation's index entry must be one the
  // schema's `#/$defs/errorCode/enum` literally lists; the contract is
  // closed, so a code in the index that the schema does not know is a hard
  // contract drift.
  const schemaEnum = readErrorCodeEnum(schema);
  for (const code of operation.errorCodes) {
    if (!schemaEnum.includes(code)) {
      throw new Error(
        `${operation.name}: error code \`${code}\` from the index is not in the schema's error-code enum ${JSON.stringify(schemaEnum)}`,
      );
    }
  }
}

/** Walk the `#/$defs/errorCode/enum` list of a per-operation schema — the
 *  flat `{type:"string", enum:[...]}` shape that `error.properties.code`
 *  points at via `$ref` in 9 of the 11 per-operation schemas. Throws if the
 *  schema is malformed.
 *
 *  The two `hub.*` schemas (MBR-701, commit 21102967) predate this flat
 *  shape: they inline `error.properties.code` as a bare `const` and carry
 *  an unreferenced, object-shaped `$defs.errorCode`
 *  (`{type:"object", properties:{code:{enum:[...]}}}`) that nothing in the
 *  schema actually validates against — a vestigial leftover, not a second
 *  valid shape. For those, fall back to the `const` that IS validated. */
function readErrorCodeEnum(schema) {
  const def = schema?.$defs?.errorCode;
  if (def && typeof def === "object" && Array.isArray(def.enum)) {
    return def.enum;
  }
  const codeConst = schema?.$defs?.error?.properties?.code?.const;
  if (codeConst !== undefined) {
    return [codeConst];
  }
  throw new Error("per-operation schema is missing #/$defs/errorCode/enum (and no #/$defs/error/properties/code const fallback)");
}

/** Read + parse a repo-relative JSON file relative to the repo root. */
export function loadRepoJson(repoRelativePath) {
  return JSON.parse(readFileSync(join(REPO_ROOT, repoRelativePath), "utf8"));
}

/** `sha256:<hex>` of an arbitrary string's UTF-8 bytes. */
export function digestStr(text) {
  return `sha256:${createHash("sha256").update(text, "utf8").digest("hex")}`;
}
