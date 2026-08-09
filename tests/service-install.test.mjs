// D15: service install — template correctness, dry-run, and target layout
// per platform (no live OS registration in tests).

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { installService, serviceTarget } from "../service/install.mjs";
import { serviceStatus } from "../service/status.mjs";
import { uninstallService } from "../service/uninstall.mjs";

const TEMPLATES = join(import.meta.dirname, "..", "service", "templates");

test("all three OS templates exist", () => {
  for (const name of ["io.orthic.cortex.plist", "cortex.service", "cortex-task.xml"]) {
    assert.ok(existsSync(join(TEMPLATES, name)), `missing template ${name}`);
  }
});

test("installService dry-run renders the platform template without writing", () => {
  const result = installService({ root: process.cwd(), dryRun: true });
  assert.ok(result.target);
  assert.ok(result.body.includes("cortex-watch.mjs"), "template must reference the watcher");
  assert.ok(result.body.includes(process.execPath), "template must embed the runtime");
});

test("macOS plist template uses foreground watcher with RunAtLoad and KeepAlive", () => {
  const body = readFileSync(join(TEMPLATES, "io.orthic.cortex.plist"), "utf8");
  assert.ok(body.includes("RunAtLoad"));
  assert.ok(body.includes("KeepAlive"));
  assert.ok(body.includes("__WATCH_SCRIPT__"));
});

test("Linux unit is a user service with restart on failure", () => {
  const body = readFileSync(join(TEMPLATES, "cortex.service"), "utf8");
  assert.ok(body.includes("[Install]"));
  assert.ok(body.includes("WantedBy=default.target"));
  assert.ok(body.includes("Restart=on-failure"));
});

test("Windows task hides the console", () => {
  const body = readFileSync(join(TEMPLATES, "cortex-task.xml"), "utf8");
  assert.ok(body.includes("Hidden"));
  assert.ok(body.includes("powershell.exe"));
});

test("serviceStatus returns a schema-valid shape", () => {
  const status = serviceStatus();
  assert.equal(status.schemaVersion, 1);
  assert.equal(status.platform, process.platform);
  assert.equal(typeof status.registered, "boolean");
  assert.ok(status.target);
});

test("uninstallService without purge preserves data", () => {
  const result = uninstallService({ purgeData: false });
  assert.equal(result.uninstalled, true);
  assert.deepEqual(result.purged, []);
});

test("uninstallService with purge lists the data dir", () => {
  const tempData = mkdtempSync(join(tmpdir(), "cortex-purge-target-"));
  try {
    const result = uninstallService({ purgeData: true, dataDir: tempData });
    assert.ok(result.purged.length > 0);
    assert.equal(result.purged[0], tempData);
  } finally {
    rmSync(tempData, { recursive: true, force: true });
  }
});
