import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { bindingFor, enroll, readRegistry, removeBinding } from "./project-registry.mjs";

const install = fileURLToPath(new URL("./install.mjs", import.meta.url));

async function invokeInstall(args, env) {
  const child = spawn(process.execPath, [install, ...args], { stdio: ["ignore", "pipe", "pipe"], windowsHide: true, env: { ...process.env, ...env } });
  let stdout = "", stderr = "";
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const code = await new Promise((resolve, reject) => { child.once("error", reject); child.once("close", resolve); });
  assert.equal(code, 0, stderr);
  return JSON.parse(stdout);
}

const root = await mkdtemp(join(tmpdir(), "membrane-registry-"));
const registry = join(root, "registry.json");
const binding = await enroll(root, { repository_id: "repo-a", scope_id: "scope-a", provider_config: { transport: "loopback" }, grant_policy: { level: "read-only" } }, registry);
assert.equal(binding.repository_id, "repo-a");
assert.equal((await bindingFor(root, registry)).scope_id, "scope-a");
await removeBinding(root, registry);
await assert.rejects(() => bindingFor(root, registry), /not enrolled/);
await enroll(root, { repository_id: "repo-a", scope_id: "scope-a" }, registry);
await writeFile(registry, "{broken", "utf8");
await assert.rejects(() => readRegistry(registry), /registry unavailable/);

const lifecycleRoot = await mkdtemp(join(tmpdir(), "membrane-install-"));
const lifecycleRegistry = join(lifecycleRoot, "registry.json");
const installEnv = { MEMBRANE_PROJECT_REGISTRY: lifecycleRegistry };
const dryEnrollment = await invokeInstall(["init", lifecycleRoot, "--repository", "repo-lifecycle", "--scope", "scope-lifecycle", "--dry-run"], installEnv);
assert.deepEqual(dryEnrollment, {
  action: "enroll", root: lifecycleRoot, repository_id: "repo-lifecycle", scope_id: "scope-lifecycle", registry: lifecycleRegistry, dry_run: true,
});
await assert.rejects(() => bindingFor(lifecycleRoot, lifecycleRegistry), /not enrolled/);

await invokeInstall(["init", lifecycleRoot, "--repository", "repo-lifecycle", "--scope", "scope-lifecycle"], installEnv);
const dryUninstall = await invokeInstall(["uninstall", lifecycleRoot, "--dry-run"], installEnv);
assert.deepEqual(dryUninstall, {
  action: "uninstall", root: lifecycleRoot, repository_id: "repo-lifecycle", registry: lifecycleRegistry, dry_run: true,
});
assert.equal((await bindingFor(lifecycleRoot, lifecycleRegistry)).repository_id, "repo-lifecycle");
await invokeInstall(["uninstall", lifecycleRoot], installEnv);
await assert.rejects(() => bindingFor(lifecycleRoot, lifecycleRegistry), /not enrolled/);
