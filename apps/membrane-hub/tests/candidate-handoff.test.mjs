import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { appendFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createPortableArchive } from "@rightkit/release/direct-bootstrap.mjs";
import { materializeCycloneDxSbom, materializeInTotoSlsaProvenance } from "@rightkit/release/supply-chain-evidence.mjs";

const hub = fileURLToPath(new URL("../", import.meta.url));
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const checker = join(hub, "scripts", "release-check-candidate-windows.mjs");
const filesUnder = (root) => readdirSync(root).flatMap((entry) => {
  const path = join(root, entry);
  return statSync(path).isDirectory() ? filesUnder(path) : [path];
});

test("candidate handoff accepts exact archive & rejects changed bytes", { skip: process.platform !== "win32" }, () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-candidate-test-"));
  try {
    const payload = join(root, "payload");
    mkdirSync(payload);
    const bytes = Buffer.from("unsigned-native-candidate\n");
    for (const name of ["membrane-hub.exe", "cortex.exe", "membrane.exe", "membrane-tray.exe", "membrane-daemon.exe"]) writeFileSync(join(payload, name), bytes);
    mkdirSync(join(payload, "runtime"));
    writeFileSync(join(payload, "runtime", "runtime.json"), bytes);
    const hookFiles = [
      "mcp/hooks/membrane-hook-entrypoint.mjs",
      "mcp/hooks/membrane-hook-runtime.mjs",
      "mcp/hooks/membrane-workspace-operations.mjs",
      "mcp/lib/verification-command.mjs",
      "mcp/lib/diagnostics-client.mjs",
      "mcp/host/context-adapter.cjs",
      "mcp/host/continuity.mjs",
      "mcp/host/delivery-ledger-store.cjs",
      "mcp/host/observable-event.cjs",
      "mcp/host/observable-ingress.cjs",
      "mcp/context-renderer-lib.cjs",
    ];
    for (const name of hookFiles) {
      mkdirSync(dirname(join(payload, name)), { recursive: true });
      writeFileSync(join(payload, name), bytes);
    }
    const archive = createPortableArchive({ sourceDir: payload, outputPath: join(root, "candidate.zip") });
    const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8", windowsHide: true }).stdout.trim();
    const namedArchive = join(root, "membrane-test-windows-x86_64-unsigned.zip");
    writeFileSync(namedArchive, readFileSync(join(root, "candidate.zip")));
    rmSync(join(root, "candidate.zip"));
    const subject = [{ name: "membrane-test-windows-x86_64-unsigned.zip", size: archive.size, sha256: archive.sha256 }];
    const sbom = join(root, "sbom-windows-x86_64-unsigned.cdx.json");
    const provenance = join(root, "provenance-windows-x86_64-unsigned.intoto.jsonl");
    const startedAt = new Date().toISOString();
    materializeCycloneDxSbom({ outputPath: sbom, product: "membrane", version: "0.0.0", target: "windows-x86_64", sourceCommit: head, files: subject });
    materializeInTotoSlsaProvenance({ outputPath: provenance, product: "membrane", version: "0.0.0", target: "windows-x86_64", sourceCommit: head, sourceRepository: "https://github.com/Orthic-Labs/Membrane", subjects: subject, startedAt });
    const evidence = [sbom, provenance].map((path) => {
      const body = readFileSync(path);
      return { name: path.split(/[\\/]/).at(-1), size: body.length, sha256: createHash("sha256").update(body).digest("hex") };
    });
    const signing = { status: "unsigned", reason: "test_fixture" };
    const installerName = "Membrane_Hub_test_unsigned_x64-setup.exe";
    const installerPath = join(root, installerName);
    writeFileSync(installerPath, bytes);
    const installerSha256 = createHash("sha256").update(bytes).digest("hex");
    const installer = { name: installerName, size: bytes.length, sha256: installerSha256, signing };
    writeFileSync(join(root, "release-manifest.json"), `${JSON.stringify({
      schema: "membrane.release-evidence.v1",
      product: "Membrane Hub",
      artifact: { path: installerName, size: bytes.length, sha256: installerSha256 },
      signing,
    })}\n`);
    writeFileSync(join(root, "sbom.json"), `${JSON.stringify({
      schema: "membrane.sbom.v1",
      artifact: { path: installerName, size: bytes.length, sha256: installerSha256 },
      signing,
    })}\n`);
    writeFileSync(join(root, "candidate.json"), `${JSON.stringify({
      schemaVersion: 1,
      kind: "membrane-unsigned-release-candidate",
      product: "membrane",
      version: "test",
      target: "windows-x86_64",
      signing,
      sourceCommit: head,
      github: { runId: "123", runAttempt: "1" },
      startedAt,
      archive: { name: "membrane-test-windows-x86_64-unsigned.zip", size: archive.size, sha256: archive.sha256 },
      installer,
      evidence,
      files: Object.fromEntries(filesUnder(payload).map((path) => [relative(payload, path).replaceAll("\\", "/"), createHash("sha256").update(readFileSync(path)).digest("hex")])),
    })}\n`);
    const exact = spawnSync(process.execPath, [checker], { cwd: repo, env: { ...process.env, RIGHT_GIT_ARTIFACT_ROOT: root }, encoding: "utf8", windowsHide: true });
    assert.equal(exact.status, 0, exact.stderr);
    appendFileSync(namedArchive, "tamper");
    const tampered = spawnSync(process.execPath, [checker], { cwd: repo, env: { ...process.env, RIGHT_GIT_ARTIFACT_ROOT: root }, encoding: "utf8", windowsHide: true });
    assert.notEqual(tampered.status, 0);
    assert.match(`${tampered.stderr}${tampered.stdout}`, /size mismatch|digest mismatch/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
