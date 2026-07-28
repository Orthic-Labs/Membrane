import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, realpath, writeFile } from "node:fs/promises";
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
assert.deepEqual((await bindingFor(root, registry)).scope_descriptor, { kind: "filesystem", path: "scope-a" });
await removeBinding(root, registry);
await assert.rejects(() => bindingFor(root, registry), /not enrolled/);
await enroll(root, { repository_id: "repo-a", scope_id: "scope-a" }, registry);
await writeFile(registry, "{broken", "utf8");
await assert.rejects(() => readRegistry(registry), /registry unavailable/);

const virtualRegistry = join(root, "virtual-registry.json");
const virtualBinding = await enroll(root, {
  repository_id: "repo-a", scope_id: "opaque-thread",
  scope_descriptor: { kind: "virtual", id: "thread:abc-123", tenant_id: "tenant-a", parents: [], inherit_global: false },
}, virtualRegistry);
assert.deepEqual(virtualBinding.scope_descriptor, { kind: "virtual", id: "thread:abc-123", tenant_id: "tenant-a", parents: [], inherit_global: false });
await assert.rejects(
  () => enroll(root, {
    repository_id: "repo-a", scope_id: "opaque-thread",
    scope_descriptor: { kind: "virtual", id: "thread:abc-123", tenant_id: "tenant-a", parents: [], inherit_global: true },
  }, virtualRegistry),
  /global inheritance/,
);
const legacyRegistry = join(root, "legacy-registry.json");
await writeFile(legacyRegistry, JSON.stringify({
  schema_version: 1,
  bindings: { [await realpath(root)]: { repository_id: "repo-legacy", scope_id: "scope-legacy" } },
}), "utf8");
assert.equal((await readRegistry(legacyRegistry)).schema_version, 2);
assert.equal(JSON.parse(await readFile(legacyRegistry, "utf8")).schema_version, 2);

const virtualInitRoot = await mkdtemp(join(tmpdir(), "membrane-virtual-init-"));
const virtualInitRegistry = join(virtualInitRoot, "registry.json");
await invokeInstall([
  "init", virtualInitRoot, "--repository", "repo-virtual", "--scope", "opaque-thread",
  "--virtual-id", "thread:abc-123", "--tenant-id", "tenant-a", "--parent", "root",
], { MEMBRANE_PROJECT_REGISTRY: virtualInitRegistry });
assert.deepEqual((await bindingFor(virtualInitRoot, virtualInitRegistry)).scope_descriptor, {
  kind: "virtual", id: "thread:abc-123", tenant_id: "tenant-a", parents: ["root"], inherit_global: false,
});

const lifecycleRoot = await mkdtemp(join(tmpdir(), "membrane-install-"));
const canonicalLifecycleRoot = await realpath(lifecycleRoot);
const lifecycleRegistry = join(lifecycleRoot, "registry.json");
const installEnv = { MEMBRANE_PROJECT_REGISTRY: lifecycleRegistry };
const dryEnrollment = await invokeInstall(["init", lifecycleRoot, "--repository", "repo-lifecycle", "--scope", "scope-lifecycle", "--dry-run"], installEnv);
assert.deepEqual(dryEnrollment, {
  action: "enroll", root: canonicalLifecycleRoot, repository_id: "repo-lifecycle", scope_id: "scope-lifecycle", registry: lifecycleRegistry, dry_run: true,
});
await assert.rejects(() => bindingFor(lifecycleRoot, lifecycleRegistry), /not enrolled/);

const configuredEnrollment = await invokeInstall(["init", lifecycleRoot, "--repository", "repo-lifecycle", "--scope", "scope-lifecycle", "--config", "mcp.json", "--native", "membrane-native"], installEnv);
assert.equal(configuredEnrollment.installation.selected, "native");
assert.equal(configuredEnrollment.installation.precedence, "native_then_config");
assert.equal((await bindingFor(lifecycleRoot, lifecycleRegistry)).provider_config.native.name, "membrane-native");
assert.equal((await bindingFor(lifecycleRoot, lifecycleRegistry)).provider_config.config.path, "mcp.json");
const configOnly = await invokeInstall(["init", lifecycleRoot, "--repository", "repo-lifecycle", "--scope", "scope-lifecycle", "--config", "mcp.json"], installEnv);
assert.equal(configOnly.installation.selected, "config");

const dryRotation = await invokeInstall(["token", "rotate", lifecycleRoot, "--dry-run"], installEnv);
assert.deepEqual(dryRotation, {
  action: "token_rotate", root: canonicalLifecycleRoot, repository_id: "repo-lifecycle", registry: lifecycleRegistry,
  token_generation: 1, revoked_token_generations: [], reason: "rotation", dry_run: true,
});
assert.equal((await bindingFor(lifecycleRoot, lifecycleRegistry)).token_grant, undefined);

const firstRotation = await invokeInstall(["token", "rotate", lifecycleRoot], installEnv);
assert.equal(firstRotation.token_generation, 1);
assert.deepEqual(firstRotation.revoked_token_generations, []);
assert.equal(Object.prototype.hasOwnProperty.call(firstRotation, "token"), false);
const afterFirstRotation = await bindingFor(lifecycleRoot, lifecycleRegistry);
assert.match(afterFirstRotation.token_grant.token_sha256, /^[a-f0-9]{64}$/);
assert.match(afterFirstRotation.token_audit[0].audit_id, /^[a-f0-9]{24}$/);
assert.equal(await readFile(afterFirstRotation.token_grant.path, "utf8").then((value) => value.trim().length > 20), true);

const recovery = await invokeInstall(["token", "recover", lifecycleRoot, "--reason", "leak"], installEnv);
assert.deepEqual(recovery.revoked_token_generations, [1]);
assert.equal(recovery.token_generation, 2);
assert.equal(recovery.reason, "leak_recovery");
assert.equal(Object.prototype.hasOwnProperty.call(recovery, "token"), false);
assert.equal((await bindingFor(lifecycleRoot, lifecycleRegistry)).token_audit.length, 2);
const dryUninstall = await invokeInstall(["uninstall", lifecycleRoot, "--dry-run"], installEnv);
assert.deepEqual(dryUninstall, {
  action: "uninstall", root: canonicalLifecycleRoot, repository_id: "repo-lifecycle", registry: lifecycleRegistry,
  revoked_token_generations: [1, 2], dry_run: true,
});
assert.equal((await bindingFor(lifecycleRoot, lifecycleRegistry)).repository_id, "repo-lifecycle");
const uninstalled = await invokeInstall(["uninstall", lifecycleRoot], installEnv);
assert.deepEqual(uninstalled.revoked_token_generations, [1, 2]);
await assert.rejects(() => bindingFor(lifecycleRoot, lifecycleRegistry), /not enrolled/);
await assert.rejects(() => readFile(afterFirstRotation.token_grant.path, "utf8"), /ENOENT/);
