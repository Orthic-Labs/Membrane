import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { buildGraphGeneration, graphCapabilities } from "../src/graph/static-provider.mjs";

function withFiles(files, run) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "blueprint-schema-native-"));
  try {
    for (const [relativePath, contents] of Object.entries(files)) {
      const absolutePath = path.join(repo, relativePath);
      fs.mkdirSync(path.dirname(absolutePath), { recursive: true });
      fs.writeFileSync(absolutePath, contents);
    }
    run(repo);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
}

test("GraphQL and SQL definitions emit schema symbols and reference relationships", () => {
  withFiles({
    "schema/catalog.graphql": [
      "type User {",
      "  id: ID!",
      "}",
      "type Product {",
      "  owner: User!",
      "}",
      "",
    ].join("\n"),
    "db/001_catalog.sql": [
      "CREATE TABLE users (id INTEGER PRIMARY KEY);",
      "CREATE TABLE products (",
      "  id INTEGER PRIMARY KEY,",
      "  owner_id INTEGER REFERENCES users(id)",
      ");",
      "",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    for (const id of [
      "symbol:schema/catalog.graphql::User",
      "symbol:schema/catalog.graphql::Product",
      "symbol:db/001_catalog.sql::users",
      "symbol:db/001_catalog.sql::products",
    ]) assert.ok(generation.nodes.some((node) => node.id === id), id);
    assert.ok(generation.edges.some((edge) => edge.kind === "REFERENCES"
      && edge.source === "symbol:schema/catalog.graphql::Product"
      && edge.target === "symbol:schema/catalog.graphql::User"));
    assert.ok(generation.edges.some((edge) => edge.kind === "REFERENCES"
      && edge.source === "symbol:db/001_catalog.sql::products"
      && edge.target === "symbol:db/001_catalog.sql::users"));
  });
});

test("NSIS and native bridge files emit symbols, includes, and calls", () => {
  withFiles({
    "installer/helpers.nsh": [
      "Function VerifyInstall",
      "FunctionEnd",
      "",
    ].join("\n"),
    "installer/setup.nsi": [
      "!include \"helpers.nsh\"",
      "Function InstallApp",
      "  Call VerifyInstall",
      "FunctionEnd",
      "",
    ].join("\n"),
    "native/bridge.h": "void start_bridge(void);\n",
    "native/bridge.mm": [
      "#include \"bridge.h\"",
      "class Recorder {};",
      "static void persist_frame(void) {}",
      "void start_bridge(void) { persist_frame(); }",
      "@implementation AppDelegate",
      "- (void)start { start_bridge(); }",
      "@end",
      "",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    for (const id of [
      "symbol:installer/helpers.nsh::VerifyInstall",
      "symbol:installer/setup.nsi::InstallApp",
      "symbol:native/bridge.h::start_bridge",
      "symbol:native/bridge.mm::Recorder",
      "symbol:native/bridge.mm::persist_frame",
      "symbol:native/bridge.mm::start_bridge",
      "symbol:native/bridge.mm::AppDelegate.start",
    ]) assert.ok(generation.nodes.some((node) => node.id === id), id);
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:installer/setup.nsi"
      && edge.target === "file:installer/helpers.nsh"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:installer/setup.nsi::InstallApp"
      && edge.target === "symbol:installer/helpers.nsh::VerifyInstall"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:native/bridge.mm"
      && edge.target === "file:native/bridge.h"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:native/bridge.mm::start_bridge"
      && edge.target === "symbol:native/bridge.mm::persist_frame"));
  });
});

test("capabilities include every first-party code and schema extension in the app repos", () => {
  const parsed = graphCapabilities().languageCoverage.parsedExtensions;
  for (const extension of ["graphql", "sql", "nsi", "nsh", "c", "h", "cc", "cpp", "cxx", "hpp", "m", "mm"]) {
    assert.ok(parsed.includes(extension), extension);
  }
});
