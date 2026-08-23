import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const SCHEMA_FILES = {
  workspaceEpoch: "../../schemas/workspace-epoch.v1.schema.json",
  coverageObligation: "../../schemas/coverage-obligation.v1.schema.json",
  evidenceSnapshot: "../../schemas/diagnostic-evidence-snapshot.v1.schema.json",
  gateDecision: "../../schemas/diagnostic-gate-decision.v1.schema.json",
};

const FIXTURE_FILES = {
  workspaceEpoch: "../../schemas/registry/workspace-epoch.v1.golden.json",
  coverageObligation: "../../schemas/registry/coverage-obligation.v1.golden.json",
  evidenceSnapshot: "../../schemas/registry/diagnostic-evidence-snapshot.v1.golden.json",
  gateDecision: "../../schemas/registry/diagnostic-gate-decision.v1.golden.json",
};

const EXPECTED_IDS = {
  workspaceEpoch: "https://membrane/schemas/workspace-epoch.v1.schema.json",
  coverageObligation: "https://membrane/schemas/coverage-obligation.v1.schema.json",
  evidenceSnapshot: "https://membrane/schemas/diagnostic-evidence-snapshot.v1.schema.json",
  gateDecision: "https://membrane/schemas/diagnostic-gate-decision.v1.schema.json",
};

const HOUSE_META_SCHEMA = "https://json-schema.org/draft/2020-12/schema";

const CLOSED_OUTCOME_VOCABULARY = [
  "clean_exact",
  "dirty_exact",
  "unknown_incomplete",
  "unknown_unavailable",
  "unknown_timed_out",
  "unknown_conflict",
  "superseded",
];

const loadJson = (repoRelative) =>
  JSON.parse(readFileSync(new URL(repoRelative, import.meta.url), "utf8"));

function resolveRef(root, ref) {
  assert.ok(ref.startsWith("#/"), `only local $refs are supported, got ${ref}`);
  let node = root;
  for (const segment of ref.slice(2).split("/")) {
    node = node[segment];
    assert.ok(node !== undefined, `unresolvable $ref ${ref}`);
  }
  return node;
}

function validateAgainstSchema(value, schema, root = schema, path = "$") {
  const errors = [];
  const fail = (message) => errors.push(`${path}: ${message}`);
  const effective = schema.$ref ? resolveRef(root, schema.$ref) : schema;
  if (schema.$ref && Object.keys(schema).length > 1) {
    return validateAgainstSchema(value, effective, root, path);
  }
  const node = effective;
  if (node.anyOf) {
    const attempts = node.anyOf.map((branch) => validateAgainstSchema(value, branch, root, path));
    if (!attempts.some((attempt) => attempt.length === 0)) {
      fail(`value matches none of anyOf (${attempts.map((a) => a.join("; ") || "ok").join(" | ")})`);
    }
  }
  if ("const" in node && !deepEqualsConst(value, node.const)) {
    fail(`expected const ${JSON.stringify(node.const)}, got ${JSON.stringify(value)}`);
  }
  if (node.enum && !node.enum.some((option) => deepEqualsConst(value, option))) {
    fail(`value ${JSON.stringify(value)} is not in enum [${node.enum.join(", ")}]`);
  }
  if (node.type) {
    const types = Array.isArray(node.type) ? node.type : [node.type];
    const matched = types.some((type) => typeMatches(value, type));
    if (!matched) fail(`expected type ${types.join("|")}, got ${typeof value}`);
  }
  if (typeof value === "string") {
    if (node.minLength !== undefined && value.length < node.minLength) fail(`shorter than minLength ${node.minLength}`);
    if (node.pattern !== undefined && !new RegExp(node.pattern).test(value)) fail(`does not match pattern ${node.pattern}`);
  }
  if ((typeof value === "number" || typeof value === "bigint") && node.minimum !== undefined && value < node.minimum) {
    fail(`below minimum ${node.minimum}`);
  }
  if (Array.isArray(value)) {
    if (Array.isArray(node.items)) {
      value.forEach((element, index) => errors.push(...validateAgainstSchema(element, node.items[index], root, `${path}[${index}]`)));
    } else if (node.items) {
      value.forEach((element, index) => errors.push(...validateAgainstSchema(element, node.items, root, `${path}[${index}]`)));
    }
  }
  if (isPlainObjectLike(value, node)) {
    for (const key of node.required ?? []) {
      if (!(key in value)) fail(`missing required property "${key}"`);
    }
    for (const [key, child] of Object.entries(node.properties ?? {})) {
      if (key in value) errors.push(...validateAgainstSchema(value[key], child, root, `${path}.${key}`));
    }
    if (node.additionalProperties === false) {
      for (const key of Object.keys(value)) {
        if (!(key in (node.properties ?? {}))) fail(`additional property "${key}" is not allowed`);
      }
    }
  }
  return errors;
}

function typeMatches(value, type) {
  switch (type) {
    case "object": return typeof value === "object" && value !== null && !Array.isArray(value);
    case "array": return Array.isArray(value);
    case "string": return typeof value === "string";
    case "boolean": return typeof value === "boolean";
    case "null": return value === null;
    case "integer": return typeof value === "number" && Number.isInteger(value);
    case "number": return typeof value === "number";
    default: return false;
  }
}

function isPlainObjectLike(value, node) {
  if (node.type !== "object") return false;
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function deepEqualsConst(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

const schemas = Object.fromEntries(Object.entries(SCHEMA_FILES).map(([key, rel]) => [key, loadJson(rel)]));
const fixtures = Object.fromEntries(Object.entries(FIXTURE_FILES).map(([key, rel]) => [key, loadJson(rel)]));

test("every live-diagnostics schema parses and follows house style", () => {
  for (const [key, schema] of Object.entries(schemas)) {
    assert.equal(schema.$schema, HOUSE_META_SCHEMA, `${key} declares the repo meta-schema`);
    assert.equal(schema.$id, EXPECTED_IDS[key], `${key} declares its canonical $id`);
    assert.equal(schema.additionalProperties, false, `${key} rejects unknown properties`);
    assert.ok(Array.isArray(schema.required) && schema.required.length > 0, `${key} lists required properties`);
  }
});

for (const [key] of Object.entries(fixtures)) {
  test(`fixture ${key} validates against its schema`, () => {
    const errors = validateAgainstSchema(fixtures[key], schemas[key]);
    assert.deepEqual(errors, []);
  });
}

test("fixtures reject schema violations", () => {
  const badOrigin = structuredClone(fixtures.workspaceEpoch);
  badOrigin.origin = "clean_partial";
  assert.ok(validateAgainstSchema(badOrigin, schemas.workspaceEpoch).length > 0, "epoch rejects unknown origin");

  const missingEpochField = structuredClone(fixtures.workspaceEpoch);
  delete missingEpochField.sourceManifestDigest;
  assert.ok(validateAgainstSchema(missingEpochField, schemas.workspaceEpoch).length > 0, "epoch requires sourceManifestDigest");

  const badDigest = structuredClone(fixtures.workspaceEpoch);
  badDigest.toolchainDigest = "sha256:NOTAHASH";
  assert.ok(validateAgainstSchema(badDigest, schemas.workspaceEpoch).length > 0, "digests follow the sha256 pattern");

  const missingIssue = structuredClone(fixtures.evidenceSnapshot);
  delete missingIssue.issues;
  assert.ok(validateAgainstSchema(missingIssue, schemas.evidenceSnapshot).length > 0, "snapshot requires issues");

  const extraSnapshotProperty = structuredClone(fixtures.evidenceSnapshot);
  extraSnapshotProperty.clean_partial = true;
  assert.ok(validateAgainstSchema(extraSnapshotProperty, schemas.evidenceSnapshot).length > 0, "snapshot rejects additional properties");

  const partialClean = structuredClone(fixtures.gateDecision);
  partialClean.outcome = "probably_clean";
  assert.ok(validateAgainstSchema(partialClean, schemas.gateDecision).length > 0, "decision rejects probably_clean");
});

test("gate decision outcome vocabulary is exactly the seven closed values", () => {
  const outcome = schemas.gateDecision.properties.outcome;
  assert.deepEqual([...outcome.enum].sort(), [...CLOSED_OUTCOME_VOCABULARY].sort());
  assert.equal(outcome.enum.length, CLOSED_OUTCOME_VOCABULARY.length);
  assert.match(outcome.description, /closed/i);
  for (const banned of ["clean_partial", "clean_stale", "probably_clean"]) {
    assert.match(outcome.description, new RegExp(banned));
    assert.ok(!outcome.enum.includes(banned), `${banned} stays outside the vocabulary`);
  }
});

test("gate decision references the snapshot and the snapshot embeds the epoch verbatim", () => {
  assert.equal(fixtures.gateDecision.snapshotId, fixtures.evidenceSnapshot.snapshotId);
  assert.deepEqual(fixtures.evidenceSnapshot.workspaceEpoch, fixtures.workspaceEpoch);
  assert.deepEqual(fixtures.gateDecision.requiredObligations, fixtures.evidenceSnapshot.coverageObligations);
  const blockingIds = new Set(fixtures.gateDecision.blockingIssueIds);
  const issueIds = new Set(fixtures.evidenceSnapshot.issues.map((issue) => issue.issueId));
  for (const id of blockingIds) assert.ok(issueIds.has(id), `blocking issue ${id} exists in the snapshot`);
});
