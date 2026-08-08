import { readJson, validateSealedManifest, validateExpectedPayloads, validateLifecycleReceipt, minimumExpectedPayloadCount } from "./contract.mjs";

const [sealedManifestPath, expectedPayloadsPath, receiptPath, tauriConfigPath] = process.argv.slice(2);
if (!sealedManifestPath || !expectedPayloadsPath || !receiptPath) {
  console.error("usage: node verify-release.mjs <release-manifest.json> <expected-payloads.json> <receipt.json> [tauri.conf.json]");
  console.error("");
  console.error("release-manifest.json is the schema:1 manifest RightKit's `right-release build");
  console.error("--platform win` (pnpm release:build:win) sealed -- never a hand-authored file.");
  console.error("expected-payloads.json is a JSON array of inner-PE names (main exe + Tauri");
  console.error("externalBin sidecars) that the NSIS installer embeds; when tauri.conf.json is");
  console.error("given, the list is required to be at least as long as bundle.externalBin + 1.");
  console.error("receipt.json proves signtool verify /pa /tw for the outer installer and every");
  console.error("expected payload, plus clean-machine install/update/uninstall gates -- none of");
  console.error("which RightKit produces. This script only validates JSON already produced");
  console.error("elsewhere; it never invokes signtool, AzureSignTool, or right-release itself.");
  process.exit(2);
}
try {
  const sealed = await readJson(sealedManifestPath);
  validateSealedManifest(sealed);
  const expectedPayloads = await readJson(expectedPayloadsPath);
  const minimumCount = tauriConfigPath ? await minimumExpectedPayloadCount(tauriConfigPath) : 1;
  validateExpectedPayloads(expectedPayloads, minimumCount);
  const receipt = await readJson(receiptPath);
  validateLifecycleReceipt(receipt, sealed, expectedPayloads);
  console.log("PASS windows release: RightKit sealed manifest + inner-PE + install/update/uninstall receipts");
} catch (error) {
  console.error(`FAIL CLOSED: ${error.message}`);
  process.exit(1);
}
