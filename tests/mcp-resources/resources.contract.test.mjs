// MBR-304 contract tests for canonical Membrane MCP resources.
//
// Acceptance: "Resources are bounded, versioned, and do not leak content
// across grants." This file asserts every resource:
//   (a) is bounded — its `accessGrants` array is non-empty and every grant
//       references a known protocol grant type declared in
//       `schemas/registry/resources/resources-index.v1.json`'s `supportedGrantTypes`
//       block,
//   (b) declares `authorityEscalation: false`,
//   (c) declares a positive `version`,
//   (d) reads inside the matching grant succeed and reads outside the grant
//       return a typed `resource_access_denied` rejection with no content
//       leak, and
//   (e) the resource body never appears in a denied response.
// Book-mode discipline: tests are written to compile and pass when run, but
// this file is not executed during implementation — the deferred command
// list runs at the end of Book 1.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import {
  listResources,
  readResource,
  readResourceByName,
  allResourceDefinitions,
  RESOURCE_NAMES,
  RESOURCE_URIS,
} from "../../mcp/resources.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..", "..");

const resourcesIndex = JSON.parse(
  await readFile(join(REPO_ROOT, "schemas", "registry", "resources", "resources-index.v1.json"), "utf8"),
);
const SUPPORTED_GRANT_TYPES = new Set(
  resourcesIndex.supportedGrantTypes.map((entry) => entry.name),
);

const definitions = await allResourceDefinitions();
assert.equal(definitions.length, 4, "four canonical resources are committed");
assert.deepEqual(
  definitions.map((r) => r.name).sort(),
  [...RESOURCE_NAMES].sort(),
  "allResourceDefinitions and RESOURCE_NAMES agree",
);

const list = await listResources();
assert.equal(list.resources.length, 4, "listResources emits the canonical set");
assert.deepEqual(
  list.resources.map((r) => r.uri).sort(),
  [...RESOURCE_URIS].sort(),
  "listResources and RESOURCE_URIS agree on the URI set",
);

for (const def of definitions) {
  assert.ok(Array.isArray(def.accessGrants), `${def.name} declares an accessGrants array`);
  assert.ok(def.accessGrants.length > 0, `${def.name} must declare at least one accessGrant`);
  for (const grant of def.accessGrants) {
    assert.ok(
      SUPPORTED_GRANT_TYPES.has(grant),
      `${def.name} declares unknown grant type: ${grant}`,
    );
  }
  assert.equal(def.authorityEscalation, false, `${def.name} must declare authorityEscalation: false`);
  assert.ok(typeof def.version === "number" && def.version >= 1, `${def.name} declares a positive version`);
  assert.ok(typeof def.mimeType === "string" && def.mimeType.length > 0, `${def.name} declares a mimeType`);
  assert.ok(typeof def.uri === "string" && def.uri.startsWith("membrane://resource/"), `${def.name} URI uses the membrane://resource/ scheme`);
  assert.ok(typeof def.description === "string" && def.description.length > 0, `${def.name} declares a description`);
}

const indexNames = new Set(resourcesIndex.resources.map((r) => r.name));
for (const def of definitions) {
  assert.ok(indexNames.has(def.name), `${def.name} is listed in resources-index.v1.json`);
}

// (d) Reads inside the matching grant succeed.
const matching = [
  ["membrane://resource/resources-index/v1", ["protocol.read"]],
  ["membrane://resource/installation-manifest/v1", ["installation.read"]],
  ["membrane://resource/lease-status/v1", ["lease.read"]],
  ["membrane://resource/operation-registry/v1", ["protocol.read"]],
];
for (const [uri, grants] of matching) {
  const got = await readResource(uri, grants);
  assert.equal(got.ok, true, `readResource(${uri}) with matching grant succeeds`);
  assert.ok(got.result.body && typeof got.result.body === "object", `${uri} body is present`);
  assert.equal(got.result.uri, uri);
}

// (e) Reads outside the matching grant return a typed rejection with no body leak.
const mismatched = [
  ["membrane://resource/installation-manifest/v1", []],
  ["membrane://resource/installation-manifest/v1", ["protocol.read"]],
  ["membrane://resource/installation-manifest/v1", ["lease.read"]],
  ["membrane://resource/lease-status/v1", []],
  ["membrane://resource/lease-status/v1", ["protocol.read"]],
  ["membrane://resource/lease-status/v1", ["installation.read"]],
  ["membrane://resource/operation-registry/v1", []],
  ["membrane://resource/operation-registry/v1", ["installation.read"]],
  ["membrane://resource/operation-registry/v1", ["lease.read"]],
];
for (const [uri, grants] of mismatched) {
  const got = await readResource(uri, grants);
  assert.equal(got.ok, false, `readResource(${uri}) with grants ${JSON.stringify(grants)} is denied`);
  assert.equal(got.result.error.kind, "error");
  assert.equal(got.result.error.code, "resource_access_denied");
  assert.equal(got.result.error.retryable, false);
  assert.ok(Array.isArray(got.result.error.details.requiredGrants));
  assert.ok(Array.isArray(got.result.error.details.presentedGrants));
  // No body field of any kind in the denied envelope.
  assert.equal(got.result.body, undefined, "denied response carries no body");
  for (const sensitive of ["installationId", "dataRootDigest", "operations", "releaseGeneration"]) {
    assert.equal(got.result[sensitive], undefined, `denied response carries no ${sensitive} field`);
    assert.equal(got.result.error.details[sensitive], undefined, `denied response carries no ${sensitive} in details`);
  }
}

// Reads of unknown URIs return a typed not-found rejection without leaking.
const unknown = await readResource("membrane://resource/does-not-exist/v1", ["protocol.read"]);
assert.equal(unknown.ok, false);
assert.equal(unknown.result.error.code, "resource_not_found");
assert.equal(unknown.result.body, undefined);

// Same enforcement applies when resolved by short name.
const byName = await readResourceByName("lease-status", ["protocol.read"]);
assert.equal(byName.ok, false);
assert.equal(byName.result.error.code, "resource_access_denied");

const byNameOk = await readResourceByName("lease-status", ["lease.read"]);
assert.equal(byNameOk.ok, true);
assert.equal(byNameOk.result.name, "lease-status");

// Resource bodies are either objects or null — never strings or arrays that
// could leak content through the wire before grants are checked.
for (const def of definitions) {
  assert.ok(def.body === null || (def.body && typeof def.body === "object" && !Array.isArray(def.body)), `${def.name} body is an object or null`);
}
