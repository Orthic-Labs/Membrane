import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { join } from "node:path";
import {
  CONTEXT_OPERATION, ENVELOPE_VERSION, ERROR_CODES, ERROR_VERSION, MembraneClient, ProtocolError, SOURCE_READ_OPERATION,
} from "../../packages/typescript/index.mjs";

const root = join(import.meta.dirname, "../..");
const read = (path) => readFile(join(root, path), "utf8");
const golden = async (name) => JSON.parse(await read(`operations/operations/${name}`));

test("canonical v1 golden envelopes share TypeScript, Python & Rust constants", async () => {
  const [context, source, python, rust] = await Promise.all([
    golden("membrane-context.v1.golden.json"), golden("membrane-source-read.v1.golden.json"),
    read("packages/python/src/membrane_client/client.py"), read("engine/crates/membrane-client/src/lib.rs"),
  ]);
  for (const envelope of [context, source]) {
    assert.equal(envelope.schemaVersion, ENVELOPE_VERSION);
    assert.equal(envelope.errorVersion, ERROR_VERSION);
  }
  assert.equal(context.operation, CONTEXT_OPERATION);
  assert.equal(source.operation, SOURCE_READ_OPERATION);
  for (const sourceText of [python, rust]) {
    assert.match(sourceText, /membrane_context/);
    assert.match(sourceText, /membrane_source_read/);
    assert.match(sourceText, /(?:_ENVELOPE_VERSION|ENVELOPE_VERSION).*1/);
    assert.match(sourceText, /(?:_ERROR_VERSION|ERROR_VERSION).*1/);
  }
});

test("TypeScript client accepts canonical success & types canonical error", async () => {
  const success = await golden("membrane-context.v1.golden.json");
  const failure = await golden("membrane-context.v1.error.golden.json");
  assert.deepEqual(new MembraneClient(() => success).context({}), success.result.data);
  assert.throws(() => new MembraneClient(() => failure).context({}), (error) => error instanceof ProtocolError && error.code === "context_deadline_exceeded" && error.retryable === true);
});

test("typed error sets match Python & Rust contracts", async () => {
  const [python, rust, index] = await Promise.all([
    read("packages/python/src/membrane_client/client.py"),
    read("engine/crates/membrane-client/src/lib.rs"),
    read("operations/operations/operations-index.v1.golden.json").then(JSON.parse),
  ]);
  for (const operation of [CONTEXT_OPERATION, SOURCE_READ_OPERATION]) {
    const codes = index.operations.find(item => item.name === operation).errorCodes;
    assert.deepEqual([...ERROR_CODES.get(operation)], codes);
    for (const code of codes) {
    assert.match(python, new RegExp(`"${code}"`));
    assert.match(rust, new RegExp(`"${code}"`));
    assert.throws(() => new MembraneClient(() => ({ schemaVersion: 1, operation, errorVersion: 1, result: { kind: "error", code, message: "typed", retryable: false } })).call(operation, {}), (error) => error instanceof ProtocolError && error.code === code);
    }
  }
});

test("clients are injected-transport only & reject version drift", () => {
  const calls = [];
  const client = new MembraneClient((operation, body) => {
    calls.push([operation, body]);
    return { schemaVersion: 1, operation, errorVersion: 1, result: { kind: "success", data: {} } };
  });
  client.sourceRead({ sourceRef: "x" });
  assert.deepEqual(calls, [[SOURCE_READ_OPERATION, { sourceRef: "x" }]]);
  assert.throws(() => new MembraneClient((operation) => ({ schemaVersion: 2, operation, errorVersion: 1, result: { kind: "success", data: {} } })).context({}), /unsupported operation envelope version/);
});
