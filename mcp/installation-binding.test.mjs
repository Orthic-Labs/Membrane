import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { installationBindingFor, installationEnv } from "./installation-binding.mjs";

const root = await mkdtemp(join(tmpdir(), "membrane-binding-"));
const workspace = join(root, "workspace");
await mkdir(join(workspace, "tools", "lib", "memory"), { recursive: true });
await mkdir(join(workspace, "tools", ".cache", "memory"), { recursive: true });
await writeFile(join(workspace, "tools", "lib", "memory", "runtime.json"), JSON.stringify({
  schemaVersion: 1,
  serviceId: "crypt-local-v1",
  host: "127.0.0.1",
  port: 47851,
}), "utf8");
await writeFile(join(workspace, "tools", ".cache", "memory", "api-token"), "workspace-token\n", "utf8");

const repoRoot = join(workspace, "membrane-repo");
await mkdir(repoRoot, { recursive: true });
const registryToken = join(root, "registry-tokens", "repo.token");
await mkdir(join(root, "registry-tokens"), { recursive: true });
await writeFile(registryToken, "registry-token\n", "utf8");

const binding = {
  root: repoRoot,
  repository_id: "repo-a",
  scope_id: "scope-a",
  token_grant: {
    generation: 2,
    token_sha256: "a".repeat(64),
    path: registryToken,
    issued_at: "2026-08-01T00:00:00.000Z",
    revoked_generations: [1],
  },
};

const resolved = await installationBindingFor(binding, { registryPath: join(root, "registry.json") });
assert.equal(resolved.workspaceRoot, workspace);
assert.equal(resolved.host, "127.0.0.1");
assert.equal(resolved.port, 47851);
assert.equal(resolved.endpoint, "http://127.0.0.1:47851");
assert.equal(resolved.db, join(workspace, "tools", ".cache", "memory", "crypt-engine.db"));
assert.equal(resolved.tokenPath, registryToken);
assert.equal(resolved.tokenGeneration, 2);

const env = installationEnv(resolved);
assert.equal(env.WORKSPACE_ROOT, workspace);
assert.equal(env.CRYPT_PORT, "47851");
assert.equal(env.CRYPT_DB, resolved.db);
assert.equal(env.CRYPT_API_TOKEN_FILE, registryToken);

process.env.CRYPT_PORT = "49123";
process.env.CRYPT_DB = "/tmp/override.db";
process.env.CRYPT_API_TOKEN_FILE = "/tmp/override.token";
const overridden = await installationBindingFor(binding);
assert.equal(overridden.port, 49123);
assert.equal(overridden.db, "/tmp/override.db");
assert.equal(overridden.tokenPath, "/tmp/override.token");
delete process.env.CRYPT_PORT;
delete process.env.CRYPT_DB;
delete process.env.CRYPT_API_TOKEN_FILE;

// F09 case 1: tools/lib/memory/runtime.json absent anywhere on the path from binding.root ->
// this is the ONLY legitimate silent-fallback case (not-installed-yet, tests, dry runs).
const bareRoot = await mkdtemp(join(tmpdir(), "membrane-binding-bare-"));
const bareRepo = join(bareRoot, "nested", "repo");
await mkdir(bareRepo, { recursive: true });
const bareBinding = { root: bareRepo, repository_id: "repo-bare", scope_id: "scope-bare" };
const bareResolved = await installationBindingFor(bareBinding, { registryPath: join(bareRoot, "registry.json") });
assert.equal(bareResolved.workspaceRoot, bareRepo);
assert.equal(bareResolved.host, "127.0.0.1");
assert.equal(bareResolved.port, 47851);
assert.equal(bareResolved.endpoint, "http://127.0.0.1:47851");
assert.equal(bareResolved.db, join(bareRepo, "tools", ".cache", "memory", "crypt-engine.db"));

// F09 case 2: runtime.json IS present but fails validation -- must fail loudly, never fall back
// to the phantom default endpoint (a stale/corrupt binding is a real production hazard).
const invalidPortRoot = await mkdtemp(join(tmpdir(), "membrane-binding-invalid-port-"));
await mkdir(join(invalidPortRoot, "tools", "lib", "memory"), { recursive: true });
await writeFile(join(invalidPortRoot, "tools", "lib", "memory", "runtime.json"), JSON.stringify({
  schemaVersion: 1, serviceId: "crypt-local-v1", host: "127.0.0.1", port: 70,
}), "utf8");
const invalidPortRepo = join(invalidPortRoot, "membrane-repo");
await mkdir(invalidPortRepo, { recursive: true });
await assert.rejects(
  () => installationBindingFor({ root: invalidPortRepo, repository_id: "repo-invalid-port", scope_id: "scope-invalid-port" }, { registryPath: join(invalidPortRoot, "registry.json") }),
  /runtime port is invalid/,
);

const mismatchRoot = await mkdtemp(join(tmpdir(), "membrane-binding-mismatch-"));
await mkdir(join(mismatchRoot, "tools", "lib", "memory"), { recursive: true });
await writeFile(join(mismatchRoot, "tools", "lib", "memory", "runtime.json"), JSON.stringify({
  schemaVersion: 1, serviceId: "some-other-service", host: "127.0.0.1", port: 47851,
}), "utf8");
const mismatchRepo = join(mismatchRoot, "membrane-repo");
await mkdir(mismatchRepo, { recursive: true });
await assert.rejects(
  () => installationBindingFor({ root: mismatchRepo, repository_id: "repo-mismatch", scope_id: "scope-mismatch" }, { registryPath: join(mismatchRoot, "registry.json") }),
  /serviceId mismatch/,
);

const nonLoopbackRoot = await mkdtemp(join(tmpdir(), "membrane-binding-non-loopback-"));
await mkdir(join(nonLoopbackRoot, "tools", "lib", "memory"), { recursive: true });
await writeFile(join(nonLoopbackRoot, "tools", "lib", "memory", "runtime.json"), JSON.stringify({
  schemaVersion: 1, serviceId: "crypt-local-v1", host: "10.0.0.5", port: 47851,
}), "utf8");
const nonLoopbackRepo = join(nonLoopbackRoot, "membrane-repo");
await mkdir(nonLoopbackRepo, { recursive: true });
await assert.rejects(
  () => installationBindingFor({ root: nonLoopbackRepo, repository_id: "repo-non-loopback", scope_id: "scope-non-loopback" }, { registryPath: join(nonLoopbackRoot, "registry.json") }),
  /runtime host must be loopback/,
);

process.stdout.write("installation-binding tests passed\n");
