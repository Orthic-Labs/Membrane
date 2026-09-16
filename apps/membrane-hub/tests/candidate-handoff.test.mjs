import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { appendFileSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
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
    // Fixture binaries are inert stubs: they prove archive/evidence closure
    // only. The authenticated MCP transport probe is explicitly skipped for
    // this fixture and the checker reports `transportProbe: "skipped"` — the
    // probe remains mandatory for real candidate gates.
    for (const name of ["membrane-hub.exe", "cortex.exe", "membrane.exe", "membrane-tray.exe", "membrane-daemon.exe", "membrane-client.exe"]) writeFileSync(join(payload, name), bytes);
    mkdirSync(join(payload, "runtime"));
    writeFileSync(join(payload, "runtime", "runtime.json"), bytes);
    for (const name of [
      "plugin.json", "mcp.json", ".mcp.json", "mcp_config.json",
      "hooks/hooks.json", "hooks/codex-hooks.json",
      ".claude-plugin/plugin.json", ".claude-plugin/marketplace.json",
      ".codex-plugin/plugin.json",
      ".antigravity-plugin/plugin.json", ".antigravity-plugin/mcp_config.json",
      ".agents/plugins/marketplace.json",
    ]) {
      mkdirSync(dirname(join(payload, name)), { recursive: true });
      writeFileSync(join(payload, name), bytes);
    }
    mkdirSync(join(payload, "skills", "membrane"), { recursive: true });
    writeFileSync(join(payload, "skills", "membrane", "SKILL.md"), bytes);
    writeFileSync(join(payload, "LICENSE"), bytes);
    writeFileSync(join(payload, "THIRD_PARTY_NOTICES.md"), bytes);
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
    writeFileSync(join(root, "release-manifest.json"), `${JSON.stringify({
      schema: "membrane.release-evidence.v1",
      product: "Membrane Hub",
      artifact: { path: "membrane-test-windows-x86_64-unsigned.zip", size: archive.size, sha256: archive.sha256 },
      signing,
    })}\n`);
    writeFileSync(join(root, "sbom.json"), `${JSON.stringify({
      schema: "membrane.sbom.v1",
      artifact: { path: "membrane-test-windows-x86_64-unsigned.zip", size: archive.size, sha256: archive.sha256 },
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
      evidence,
      files: Object.fromEntries(filesUnder(payload).map((path) => [relative(payload, path).replaceAll("\\", "/"), createHash("sha256").update(readFileSync(path)).digest("hex")])),
    })}\n`);
    const env = { ...process.env, RIGHT_GIT_ARTIFACT_ROOT: root, MEMBRANE_CANDIDATE_TRANSPORT_PROBE: "0" };
    const exact = spawnSync(process.execPath, [checker], { cwd: repo, env, encoding: "utf8", windowsHide: true });
    assert.equal(exact.status, 0, exact.stderr);
    // The skip is explicit in the result payload — a fixture never silently
    // claims transport proof.
    assert.match(exact.stdout, /"transportProbe":\s*"skipped"/);
    const intact = readFileSync(namedArchive);
    appendFileSync(namedArchive, "tamper");
    const tampered = spawnSync(process.execPath, [checker], { cwd: repo, env, encoding: "utf8", windowsHide: true });
    assert.notEqual(tampered.status, 0);
    assert.match(`${tampered.stderr}${tampered.stdout}`, /size mismatch|digest mismatch/);
    // Without the explicit skip the probe must run — and fail — because the
    // staged binaries are inert fixture bytes, not a working engine. The
    // archive is restored first so the failure is the probe, not the digest.
    writeFileSync(namedArchive, intact);
    const probed = spawnSync(process.execPath, [checker], {
      cwd: repo,
      env: { ...process.env, RIGHT_GIT_ARTIFACT_ROOT: root, MEMBRANE_CANDIDATE_TRANSPORT_PROBE: "1" },
      encoding: "utf8",
      windowsHide: true,
      timeout: 150_000,
    });
    assert.notEqual(probed.status, 0, "fixture binaries must fail the transport probe");
    assert.match(`${probed.stderr}${probed.stdout}`, /candidate engine|transport|MCP|mcp/i);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
