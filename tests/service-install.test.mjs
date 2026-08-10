// D15: service lifecycle — no OS registration (D-S03), foreground mode only (D-S04)

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { installService, serviceTarget, foregroundRunArgs } from "../service/install.mjs";
import { serviceStatus } from "../service/status.mjs";
import { uninstallService } from "../service/uninstall.mjs";

test("OS templates are deleted per D-17", () => {
  const templates = join(import.meta.dirname, "..", "service", "templates");
  assert.equal(existsSync(templates), false, "service/templates must not exist per D-17");
});

test("installService dry-run returns foreground argv, no OS target", () => {
  const result = installService({ root: process.cwd(), dryRun: true });
  assert.equal(result.target, null);
  assert.ok(Array.isArray(result.serviceStart));
  assert.ok(result.serviceStart.includes("cortex-watch.mjs") || result.serviceStart.join(" ").includes("cortex-watch"));
  assert.ok(result.forbidden || result.body.includes("D-S03") || result.body.includes("Hub-owned"));
});

test("installService forbids OS registration", () => {
  const result = installService({ root: process.cwd(), dryRun: false });
  assert.equal(result.installed, false);
  assert.equal(result.target, null);
  assert.ok(result.note.includes("D-S03") || result.note.includes("Hub"));
});

test("foregroundRunArgs returns cortex service run target", () => {
  const args = foregroundRunArgs();
  assert.ok(Array.isArray(args));
  assert.ok(args.join(" ").includes("cortex-watch.mjs"));
});

test("serviceTarget is null per D-S03", () => {
  assert.equal(serviceTarget(), null);
});

test("serviceStatus returns Hub-owned shape, no OS registration", () => {
  const status = serviceStatus();
  assert.equal(status.schemaVersion, 1);
  assert.equal(status.platform, process.platform);
  assert.equal(status.registered, false);
  assert.equal(status.target, null);
  assert.equal(status.foreground, "cortex service run");
});

test("uninstallService without purge preserves data", () => {
  const result = uninstallService({ purgeData: false });
  assert.equal(result.uninstalled, true);
  assert.deepEqual(result.purged, []);
  assert.equal(result.target, null);
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

test("grep gate: no OS-registration code reachable in service/", async () => {
  const { readFileSync, readdirSync } = await import("node:fs");
  const { join } = await import("node:path");
  const svcFiles = readdirSync("service").filter((f) => f.endsWith(".mjs")).map((f) => join("service", f));
  for (const f of svcFiles) {
    const src = readFileSync(f, "utf8");
    assert.equal(/launchctl/.test(src), false, `${f} must not contain launchctl per D-S03`);
    assert.equal(/systemd/.test(src), false, `${f} must not contain systemd per D-S03`);
    assert.equal(/schtasks/.test(src), false, `${f} must not contain schtasks per D-S03`);
    assert.equal(/LaunchAgent/.test(src), false, `${f} must not contain LaunchAgent per D-S03`);
  }
});
