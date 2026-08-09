// D02 contract catalog freeze: every public/compatibility contract is
// fixture-backed and schema-validated via lib/contracts/validate.mjs.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { CONTRACT_CATALOG, contractByName } from "../lib/contracts/catalog.mjs";
import { validateContract, validateContractResult } from "../lib/contracts/validate.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const GOLDENS = join(ROOT, "schemas/goldens");
const CURRENT = join(GOLDENS, "current");
const COMPAT = join(GOLDENS, "compat", "0.2.0");

function loadJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

test("catalog contains exactly the fifteen frozen contracts", () => {
  assert.equal(CONTRACT_CATALOG.length, 15);
  const names = CONTRACT_CATALOG.map((entry) => entry.name);
  assert.equal(new Set(names).size, 15, "duplicate catalog name");
  for (const entry of CONTRACT_CATALOG) {
    assert.ok(entry.version === 1, `${entry.name} must be version 1`);
    assert.ok(entry.file.endsWith(".schema.json"), `${entry.name} file is a schema`);
    const schemaPath = join(ROOT, "schemas", entry.file);
    assert.ok(readFileSync(schemaPath, "utf8").length > 0, `${entry.file} must exist and be non-empty`);
  }
});

test("no duplicate schema $id across the catalog", () => {
  const ids = CONTRACT_CATALOG.map((entry) => {
    const schema = loadJson(join(ROOT, "schemas", entry.file));
    return schema.$id;
  });
  assert.equal(new Set(ids).size, ids.length, `duplicate $id: ${ids.join(", ")}`);
});

test("every current golden validates against its contract", () => {
  const files = readdirSync(CURRENT).filter((f) => f.endsWith(".golden.json"));
  assert.equal(files.length, CONTRACT_CATALOG.length, "one current golden per contract");
  for (const entry of CONTRACT_CATALOG) {
    const goldenName = entry.file.replace(".schema.json", ".golden.json");
    const payload = loadJson(join(CURRENT, goldenName));
    const result = validateContractResult(entry.name, payload);
    assert.equal(result.ok, true, `${entry.name}: ${JSON.stringify(result.error ?? null)}`);
  }
});

test("every compatibility golden is readable and schema-valid", () => {
  const files = readdirSync(COMPAT).filter((f) => f.endsWith(".compat.json"));
  assert.ok(files.length >= 5, "at least the five original contracts have compat fixtures");
  for (const file of files) {
    // Compat filenames use the schema file basename (e.g. context-candidate-set-v1);
    // map back to the catalog entry by schema file.
    const schemaFile = `${file.replace(/\.compat\.json$/, "")}.schema.json`;
    const entry = CONTRACT_CATALOG.find((item) => item.file === schemaFile);
    assert.ok(entry, `no catalog entry for ${file}`);
    const payload = loadJson(join(COMPAT, file));
    const result = validateContractResult(entry.name, payload);
    assert.equal(result.ok, true, `${file}: ${JSON.stringify(result.error ?? null)}`);
  }
});

test("each contract rejects one malformed fixture with contract_invalid", () => {
  for (const entry of CONTRACT_CATALOG) {
    const goldenName = entry.file.replace(".schema.json", ".golden.json");
    const payload = loadJson(join(CURRENT, goldenName));
    const schema = loadJson(join(ROOT, "schemas", entry.file));
    const required = schema.required ?? [];
    assert.ok(required.length > 0, `${entry.name} schema must declare required fields`);
    const malformed = { ...payload };
    delete malformed[required[0]];
    const result = validateContractResult(entry.name, malformed);
    assert.equal(result.ok, false, `${entry.name} should reject malformed fixture`);
    assert.equal(result.error.error.code, "contract_invalid", `${entry.name} error code`);
    assert.ok(result.error.error.schemaId, `${entry.name} error carries schema id`);
    assert.ok(typeof result.error.error.pointer === "string", `${entry.name} error carries instance pointer`);
  }
});

test("validateContract throws contract_invalid with details", () => {
  const entry = CONTRACT_CATALOG[0];
  const goldenName = entry.file.replace(".schema.json", ".golden.json");
  const payload = loadJson(join(CURRENT, goldenName));
  delete payload[Object.keys(payload)[0]];
  assert.throws(
    () => validateContract(entry.name, payload),
    (error) => error.code === "contract_invalid" && error.details?.code === "contract_invalid",
  );
});

test("unknown contract name throws contract_unknown", () => {
  assert.throws(
    () => contractByName("NoSuchContractV1"),
    (error) => error.code === "contract_unknown",
  );
});
