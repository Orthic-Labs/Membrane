import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = path => readFileSync(join(root, path), "utf8");

test("retired install workspace projection cannot return", () => {
  for (const path of ["install/workspace", "dist/install/workspace"]) {
    if (existsSync(join(root, path))) assert.deepEqual(readdirSync(join(root, path), { recursive: true }), [], path);
  }
  for (const path of [
    "scripts/generate-install-workspace.mjs",
    "scripts/generate-install-workspace.test.mjs",
  ]) assert.equal(existsSync(join(root, path)), false, path);
});

test("development Hub cannot become installed product or mutate global bindings", () => {
  const dev = read("apps/membrane-hub/scripts/dev.mjs");
  for (const term of [
    'MEMBRANE_RUNTIME_ORIGIN: "development"',
    "MEMBRANE_DEV_ROOT",
    "MEMBRANE_DEV_PORT",
    '"Orthic Labs", "Membrane Dev"',
  ]) assert.ok(dev.includes(term), term);
  assert.doesNotMatch(
    dev,
    /\bactivate\b|--install-root|activation-receipt|\bcurrent\b|\.cursor[\\/]mcp\.json|\.codeium[\\/]windsurf|\.gemini[\\/]config[\\/]mcp_config\.json|\bmcp\s+(?:add|remove)\b/i,
  );
  const enrollment = read("mcp/install.mjs");
  assert.doesNotMatch(enrollment, /\?\s*createInstalledNativeInstaller\([^)]*\)\s*:\s*createNativeInstaller/);
  assert.match(enrollment, /MEMBRANE_RUNTIME_ORIGIN !== "installed"/);
  assert.match(enrollment, /global client binding requires installed runtime origin, stable current, and activation identity receipt/);
});

test("installed activation is stable-current only & persists identity receipt", () => {
  const activation = read("engine/crates/membrane/src/activation.rs");
  for (const term of [
    'join("Orthic Labs").join("Membrane").join("current")',
    "activation install root must be exact stable current path",
    "repository, dist, target, node_modules, and version-specific roots are prohibited",
    "std::fs::read_link(&stable)",
    "ACTIVATION_RECEIPT_FILE",
    "persist_receipt(&workspace_root, &receipt)?",
    "runtime_origin: RuntimeOrigin::Installed",
    "release_generation",
  ]) assert.ok(activation.includes(term), term);
  const enrollment = read("mcp/install.mjs");
  for (const term of [
    'join(dirname(root), "state", "activation-receipt.json")',
    'receipt.runtimeOrigin !== "installed"',
    "receipt.dryRun !== false",
    "sameInstalledPath(receipt.installRoot, root, platform)",
    "sameInstalledPath(receipt.membraneExecutable, executable, platform)",
    'receipt.service?.serviceId !== "membrane-hub"',
    "installedActivationReceipt({ env, platform });",
  ]) assert.ok(enrollment.includes(term), term);
});

test("installed dogfood binds stable current to installation identity evidence", () => {
  const bootstrap = read("apps/membrane-hub/scripts/finalize-portable-release.mjs");
  assert.match(bootstrap, /activationArgs:\s*\["activate", "--install-root", "\{current\}"\]/);
  assert.match(bootstrap, /statusArgs:\s*\["activate", "--install-root", "\{current\}", "--dry-run"\]/);
  for (const assertion of [
    '{ path: "runtimeOrigin", equals: "installed" }',
    '{ path: "installRoot", nonempty: true }',
    '{ path: "versionRoot", nonempty: true }',
    '{ path: "membraneExecutable", nonempty: true }',
    '{ path: "trayExecutable", nonempty: true }',
    '{ path: "service.releaseGeneration", nonempty: true }',
  ]) assert.ok(bootstrap.includes(assertion), assertion);

  const qualification = read("scripts/qualification/install-release.ps1");
  assert.match(qualification, /Orthic Labs\\Membrane\\current/);
  assert.match(qualification, /install root must be stable Membrane current path/);
  assert.match(qualification, /'installationId', 'cortexStoreId', 'releaseGeneration'/);
  assert.match(qualification, /Write-JsonAtomic \$EvidencePath \$receipt/);
});
