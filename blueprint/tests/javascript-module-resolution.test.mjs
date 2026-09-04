import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { resolveModuleSpecifier } from "../src/providers/modules/javascript.mjs";

function repo() {
  const root = mkdtempSync(join(tmpdir(), "blueprint-js-resolution-"));
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src", "main.ts"), "export {};\n");
  return root;
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

test("tsconfig paths/baseUrl resolve before package lookup and tolerate JSONC", () => {
  const root = repo();
  try {
    mkdirSync(join(root, "src", "lib"), { recursive: true });
    writeFileSync(join(root, "src", "lib", "retry.ts"), "export const retry = true;\n");
    writeFileSync(join(root, "tsconfig.json"), `{
      // project aliases are source truth for TypeScript resolution
      "compilerOptions": {
        "baseUrl": ".",
        "paths": {
          "@lib/*": ["src/lib/*"],
        },
      },
    }\n`);

    const result = resolveModuleSpecifier({
      specifier: "@lib/retry",
      fromFile: join(root, "src", "main.ts"),
      repoRoot: root,
    });
    assert.equal(result.status, "RESOLVED");
    assert.equal(result.reason, "tsconfig_paths");
    assert.equal(result.resolved, join(root, "src", "lib", "retry.ts"));
    assert.equal(result.configPath, join(root, "tsconfig.json"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("package exports honors conditions and subpaths instead of falling through to main", () => {
  const root = repo();
  try {
    const pkg = join(root, "node_modules", "modern-pkg");
    mkdirSync(join(pkg, "dist"), { recursive: true });
    writeJson(join(pkg, "package.json"), {
      name: "modern-pkg",
      main: "legacy.js",
      exports: {
        ".": { types: "./dist/index.d.ts", import: "./dist/index.js", default: "./dist/index.js" },
        "./feature/*": "./dist/feature/*.js",
      },
    });
    writeFileSync(join(pkg, "legacy.js"), "module.exports = {};\n");
    writeFileSync(join(pkg, "dist", "index.d.ts"), "export declare const value: number;\n");
    writeFileSync(join(pkg, "dist", "index.js"), "export const value = 1;\n");
    mkdirSync(join(pkg, "dist", "feature"), { recursive: true });
    writeFileSync(join(pkg, "dist", "feature", "retry.js"), "export const retry = true;\n");

    const rootImport = resolveModuleSpecifier({
      specifier: "modern-pkg",
      fromFile: join(root, "src", "main.ts"),
      conditions: ["import", "node", "default"],
    });
    assert.equal(rootImport.status, "RESOLVED");
    assert.equal(rootImport.reason, "package_exports");
    assert.equal(rootImport.resolved, join(pkg, "dist", "index.js"));

    const subpath = resolveModuleSpecifier({
      specifier: "modern-pkg/feature/retry",
      fromFile: join(root, "src", "main.ts"),
      conditions: ["import", "default"],
    });
    assert.equal(subpath.status, "RESOLVED");
    assert.equal(subpath.resolved, join(pkg, "dist", "feature", "retry.js"));

    const hidden = resolveModuleSpecifier({
      specifier: "modern-pkg/not-exported",
      fromFile: join(root, "src", "main.ts"),
    });
    assert.equal(hidden.status, "UNRESOLVED");
    assert.equal(hidden.reason, "package_exports_unresolved");
    assert.notEqual(hidden.resolved, join(pkg, "legacy.js"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("package imports resolve declared internal aliases", () => {
  const root = repo();
  try {
    mkdirSync(join(root, "src", "internal"), { recursive: true });
    writeFileSync(join(root, "src", "internal", "retry.ts"), "export const retry = true;\n");
    writeJson(join(root, "package.json"), {
      name: "app",
      imports: { "#internal/*": "./src/internal/*.ts" },
    });

    const result = resolveModuleSpecifier({
      specifier: "#internal/retry",
      fromFile: join(root, "src", "main.ts"),
      repoRoot: root,
    });
    assert.equal(result.status, "RESOLVED");
    assert.equal(result.reason, "package_imports");
    assert.equal(result.resolved, join(root, "src", "internal", "retry.ts"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("workspace package identity resolves to repository source", () => {
  const root = repo();
  try {
    writeJson(join(root, "package.json"), { private: true, workspaces: ["packages/*"] });
    const pkg = join(root, "packages", "shared");
    mkdirSync(join(pkg, "src"), { recursive: true });
    writeJson(join(pkg, "package.json"), {
      name: "@acme/shared",
      exports: { ".": "./src/index.ts", "./retry": "./src/retry.ts" },
    });
    writeFileSync(join(pkg, "src", "index.ts"), "export {};\n");
    writeFileSync(join(pkg, "src", "retry.ts"), "export const retry = true;\n");

    const result = resolveModuleSpecifier({
      specifier: "@acme/shared/retry",
      fromFile: join(root, "src", "main.ts"),
      repoRoot: root,
    });
    assert.equal(result.status, "RESOLVED");
    assert.equal(result.workspacePackage, "@acme/shared");
    assert.equal(result.resolved, join(pkg, "src", "retry.ts"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("node_modules dependency becomes explicit external identity when repository scope is known", () => {
  const root = repo();
  try {
    const pkg = join(root, "node_modules", "external-pkg");
    mkdirSync(pkg, { recursive: true });
    writeJson(join(pkg, "package.json"), { name: "external-pkg", main: "index.js" });
    writeFileSync(join(pkg, "index.js"), "module.exports = {};\n");

    const scoped = resolveModuleSpecifier({
      specifier: "external-pkg",
      fromFile: join(root, "src", "main.ts"),
      repoRoot: root,
    });
    assert.equal(scoped.status, "EXTERNAL");
    assert.deepEqual(scoped.externalPackage, { packageName: "external-pkg", specifier: "external-pkg" });

    const legacyUnscoped = resolveModuleSpecifier({
      specifier: "external-pkg",
      fromFile: join(root, "src", "main.ts"),
    });
    assert.equal(legacyUnscoped.status, "RESOLVED");
    assert.equal(legacyUnscoped.resolved, join(pkg, "index.js"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});
