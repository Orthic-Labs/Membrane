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
  serviceId: "memright-local-v1",
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
assert.equal(resolved.db, join(workspace, "tools", ".cache", "memory", "memright-engine.db"));
assert.equal(resolved.tokenPath, registryToken);
assert.equal(resolved.tokenGeneration, 2);

const env = installationEnv(resolved);
assert.equal(env.WORKSPACE_ROOT, workspace);
assert.equal(env.MEMRIGHT_PORT, "47851");
assert.equal(env.MEMRIGHT_DB, resolved.db);
assert.equal(env.MEMRIGHT_API_TOKEN_FILE, registryToken);

process.env.MEMRIGHT_PORT = "49123";
process.env.MEMRIGHT_DB = "/tmp/override.db";
process.env.MEMRIGHT_API_TOKEN_FILE = "/tmp/override.token";
const overridden = await installationBindingFor(binding);
assert.equal(overridden.port, 49123);
assert.equal(overridden.db, "/tmp/override.db");
assert.equal(overridden.tokenPath, "/tmp/override.token");
delete process.env.MEMRIGHT_PORT;
delete process.env.MEMRIGHT_DB;
delete process.env.MEMRIGHT_API_TOKEN_FILE;

process.stdout.write("installation-binding tests passed\n");
