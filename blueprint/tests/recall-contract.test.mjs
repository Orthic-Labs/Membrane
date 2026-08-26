import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { METHODS } from "../src/service/protocol.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { createAdmission } from "../src/lib/admission.mjs";

test("Blueprint exposes recall as its sole context-admission operation", () => {
  assert.ok(METHODS.includes("recall"));
  assert.equal(METHODS.includes("orient"), false);
  const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
  assert.equal(typeof service.recall, "function");
  assert.equal("orient" in service, false);
  const admission = createAdmission({ readGeneration: () => null, createContextCandidateSet: () => null });
  assert.equal(typeof admission.recall, "function");
  assert.equal("orient" in admission, false);
});

test("packaging has no retired Blueprint executable alias", () => {
  const packageJson = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  assert.equal(packageJson.bin["membrane-blueprint"], undefined);
});
