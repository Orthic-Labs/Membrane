import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { verifyWindowsChannels } from "../../../scripts/release/verify-windows-channels.mjs";

const contractPath = "packaging/winget/release.v1.json";

test("Windows channel source contract cannot publish or install placeholders", async () => {
  const [contract, scoop] = await Promise.all([readFile(contractPath, "utf8").then(JSON.parse), readFile("packaging/scoop/membrane.json", "utf8").then(JSON.parse)]);
  assert.deepEqual(await verifyWindowsChannels(contract), { status: "unavailable" });
  assert.equal(contract.publish, false);
  assert.equal(contract.artifact.url, null);
  assert.equal(contract.channels.winget.installerSha256, null);
  assert.equal(contract.channels.scoop.url, null);
  assert.equal(scoop.version, "0.0.0-unavailable");
  assert.equal("url" in scoop, false);
  assert.equal("hash" in scoop, false);
});

test("ready channels bind one signed NSIS artifact to Winget & Scoop", async () => {
  const value = JSON.parse(await readFile(contractPath, "utf8"));
  value.state = "ready";
  value.sourceIdentity = { commit: "a".repeat(40), version: "v1.2.3", releaseGeneration: "b".repeat(64) };
  value.artifact.name = "Membrane_1.2.3_x64-setup.exe";
  value.artifact.url = "https://downloads.example/Membrane_1.2.3_x64-setup.exe";
  value.artifact.sha256 = "c".repeat(64);
  const root = await mkdtemp(join(tmpdir(), "membrane-windows-channel-"));
  const receipt = { schema: "windows-installer-receipt.v1", status: "pass", source_commit: value.sourceIdentity.commit, installer_sha256: value.artifact.sha256, gates: { signature: "pass", install: "pass", update: "pass", uninstall: "pass" } };
  const bytes = JSON.stringify(receipt); await writeFile(join(root, "receipt.json"), bytes);
  value.trust = { receipt: { path: "receipt.json", sha256: createHash("sha256").update(bytes).digest("hex") } };
  value.channels.winget.installerUrl = value.artifact.url;
  value.channels.winget.installerSha256 = value.artifact.sha256;
  value.channels.scoop.url = value.artifact.url;
  value.channels.scoop.sha256 = value.artifact.sha256;
  assert.equal((await verifyWindowsChannels(value, root)).status, "ready");
  value.channels.scoop.sha256 = "d".repeat(64);
  await assert.rejects(() => verifyWindowsChannels(value, root), /Scoop artifact binding/);
});
