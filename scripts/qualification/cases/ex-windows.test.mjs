import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as cases from "./ex-windows.mjs";

const REQUIRED_EX_IDS = ["EX-01", "EX-02", "EX-03", "EX-04", "EX-05", "EX-06", "EX-07", "EX-08", "EX-09"];

function mkFixtureSourceRoot() {
  const root = mkdtempSync(join(tmpdir(), "ex-windows-src-"));
  mkdirSync(join(root, "engine/crates/membrane-core/src"), { recursive: true });
  writeFileSync(join(root, "engine/crates/membrane-core/src/lib.rs"), "// clean file\n");
  mkdirSync(join(root, "schemas/registry/operations"), { recursive: true });
  writeFileSync(join(root, "schemas/registry/operations/ops.json"), "{}\n");
  return root;
}

// A "real" installed root fixture: canonical executables + a clean mcp.json
// + a clean mcp/ surface. Executables are dummy (non-runnable) files, so any
// probe that actually invokes them (EX-01's `membrane.exe status`, which
// this fixture cannot answer) is expected to come back "insufficient", never
// a fabricated pass — this is exercised explicitly below.
function mkFixtureInstalledRoot({ mcpServers = { membrane: { type: "stdio", command: "membrane" } }, extraExecutable = null, forbiddenInMcpDir = null, forbiddenInBinary = null } = {}) {
  const root = mkdtempSync(join(tmpdir(), "ex-windows-installed-"));
  for (const exe of ["cortex.exe", "membrane.exe", "membrane-daemon.exe", "membrane-hub.exe", "membrane-tray.exe"]) {
    writeFileSync(join(root, exe), forbiddenInBinary && exe === "membrane.exe" ? forbiddenInBinary : "MZ-fixture-binary-not-runnable");
  }
  if (extraExecutable) writeFileSync(join(root, extraExecutable), "MZ-fixture-binary-not-runnable");
  writeFileSync(join(root, "mcp.json"), JSON.stringify({ mcpServers }, null, 2));
  writeFileSync(join(root, "plugin.json"), JSON.stringify({ name: "membrane" }, null, 2));
  writeFileSync(join(root, "release.json"), JSON.stringify({ product: "membrane" }, null, 2));
  mkdirSync(join(root, "mcp"), { recursive: true });
  writeFileSync(join(root, "mcp/install.mjs"), forbiddenInMcpDir ? forbiddenInMcpDir : "// clean installed surface\n");
  return root;
}

test("every required EX export exists and is callable", () => {
  for (const id of REQUIRED_EX_IDS) {
    const exportName = id.replace("-", "_");
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
  }
});

test("static half unchanged: a clean fixture source tree with no installed root reports insufficient (unclaimed runtime), never a fabricated pass", () => {
  const srcRoot = mkFixtureSourceRoot();
  try {
    const result = cases.EX_01({ root: srcRoot, installedRoot: join(srcRoot, "does-not-exist") });
    assert.equal(result.status, "insufficient");
    assert.equal(result.evidenceKind, "source");
    assert.match(result.reason, /runtime half remains unclaimed/);
    assert.match(result.reason, /does-not-exist/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
  }
});

test("negative control: a forbidden static pattern in the source fixture still fails regardless of any installed root", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot();
  try {
    writeFileSync(join(srcRoot, "engine/crates/membrane-core/src/lib.rs"), "struct FooGraphService {}\n");
    const result = cases.EX_01({ root: srcRoot, installedRoot });
    assert.equal(result.status, "failed");
    assert.match(result.reason, /forbidden pattern present/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("runtime half passes for a generic EX case (no dedicated service/process/schedule probe) against a clean fixture installed root", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot();
  try {
    const result = cases.EX_04({ root: srcRoot, installedRoot });
    assert.equal(result.status, "passed");
    assert.equal(result.evidenceKind, "installed");
    assert.equal(result.detail.installedRuntime.hits.length, 0);
    assert.match(result.reason, /Runtime half also passed/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("negative control: a forbidden pattern in the installed candidate's shipped mcp/ surface fails the runtime half even though source is clean", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot({ forbiddenInMcpDir: "const x = new MarkdownGraphStore();\n" });
  try {
    const result = cases.EX_04({ root: srcRoot, installedRoot });
    assert.equal(result.status, "failed");
    assert.equal(result.evidenceKind, "installed");
    assert.match(result.reason, /installed-candidate runtime evidence found a forbidden condition/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("negative control: a forbidden literal in a shipped executable's raw bytes fails the runtime half (binary scan)", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot({ forbiddenInBinary: "junk-bytes-SpeculativeBranch-more-junk" });
  try {
    const result = cases.EX_08({ root: srcRoot, installedRoot });
    assert.equal(result.status, "failed");
    assert.match(result.reason, /binary:SpeculativeBranch/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("EX-01: a non-canonical extra executable in the installed root is a runtime failure", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot({ extraExecutable: "membrane-graph-service.exe" });
  try {
    const result = cases.EX_01({ root: srcRoot, installedRoot });
    assert.equal(result.status, "failed");
    assert.match(result.reason, /non-canonical installed executable/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("EX-01: a second registered mcpServers entry in the installed mcp.json is a runtime failure", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot({ mcpServers: { membrane: { type: "stdio" }, "membrane-graph": { type: "stdio" } } });
  try {
    const result = cases.EX_01({ root: srcRoot, installedRoot });
    assert.equal(result.status, "failed");
    assert.match(result.reason, /non-canonical mcpServers entry/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("EX-01: when the installed membrane.exe cannot answer the live service-identity probe, the result is a typed insufficient, never a fabricated pass", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot();
  try {
    const result = cases.EX_01({ root: srcRoot, installedRoot });
    // The fixture membrane.exe is not a real, runnable binary, so the live
    // `status --dry-run` probe cannot be answered from this fixture.
    assert.equal(result.status, "insufficient");
    assert.match(result.reason, /Runtime half insufficient/);
    assert.match(result.reason, /service-identity probe/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("EX-09: an automatic-paid-refresh literal shipped in the installed mcp/ surface is a runtime failure", () => {
  const srcRoot = mkFixtureSourceRoot();
  const installedRoot = mkFixtureInstalledRoot({ forbiddenInMcpDir: "auto_paid_refresh(config);\n" });
  try {
    const result = cases.EX_09({ root: srcRoot, installedRoot });
    assert.equal(result.status, "failed");
    assert.match(result.reason, /installed-candidate runtime evidence found a forbidden condition/);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(installedRoot, { recursive: true, force: true });
  }
});

test("every EX case is callable against the live repository and the live MEMBRANE_QUALIFICATION_INSTALLED_ROOT (if configured) without throwing", () => {
  for (const id of REQUIRED_EX_IDS) {
    const exportName = id.replace("-", "_");
    const result = cases[exportName]();
    assert.equal(result.detail.id, id);
    assert.ok(["passed", "failed", "insufficient"].includes(result.status), `${id} returned unrecognized status ${result.status}`);
    assert.ok(["source", "installed"].includes(result.evidenceKind), `${id} returned unrecognized evidenceKind ${result.evidenceKind}`);
    assert.ok(typeof result.reason === "string" && result.reason.length > 0, `${id} returned no reason`);
  }
});
