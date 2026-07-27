#!/usr/bin/env node
// Project-scoped enrollment only. It never edits rules files or global client config.
import { bindingFor, canonicalRoot, defaultRegistryPath, enroll, removeBinding } from "./project-registry.mjs";

function usage() { return "usage: membrane init <root> --repository <id> --scope <id> [--dry-run] | membrane uninstall <root> [--dry-run]"; }
const [, , command, root, ...rest] = process.argv;
const value = (flag) => { const i = rest.indexOf(flag); return i >= 0 ? rest[i + 1] : undefined; };
const dryRun = rest.includes("--dry-run");
if (!command || !root) throw new Error(usage());
if (command === "init") {
  const repository_id = value("--repository");
  const scope_id = value("--scope");
  if (!repository_id || !scope_id) throw new Error(usage());
  const canonical = await canonicalRoot(root);
  const receipt = { action: "enroll", root: canonical, repository_id, scope_id, registry: defaultRegistryPath(), dry_run: dryRun };
  if (!dryRun) Object.assign(receipt, await enroll(canonical, { repository_id, scope_id, provider_config: { transport: "loopback" }, grant_policy: { level: "read-only" } }));
  process.stdout.write(JSON.stringify(receipt) + "\n");
} else if (command === "uninstall") {
  const binding = await bindingFor(root);
  const receipt = { action: "uninstall", root: binding.root, repository_id: binding.repository_id, registry: defaultRegistryPath(), dry_run: dryRun };
  if (!dryRun) await removeBinding(root);
  process.stdout.write(JSON.stringify(receipt) + "\n");
} else {
  throw new Error(usage());
}
