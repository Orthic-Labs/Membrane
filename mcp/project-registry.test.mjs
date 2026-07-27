import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { bindingFor, enroll, readRegistry, removeBinding } from "./project-registry.mjs";

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
