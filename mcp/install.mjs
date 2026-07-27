#!/usr/bin/env node
// Project-scoped enrollment only. It never edits rules files or global client config.
import { bindingFor, canonicalRoot, defaultRegistryPath, enroll, installationFor, removeBinding, rotateToken } from "./project-registry.mjs";

function usage() { return "usage: membrane init <root> --repository <id> --scope <id> [--native <name>] [--config <path>] [--dry-run] | membrane uninstall <root> [--dry-run] | membrane token rotate <root> [--dry-run] | membrane token recover <root> --reason leak [--dry-run]"; }
const [, , command, ...args] = process.argv;
const value = (items, flag) => { const i = items.indexOf(flag); return i >= 0 ? items[i + 1] : undefined; };
const dryRunFor = (items) => items.includes("--dry-run");

async function init(root, rest) {
  const repository_id = value(rest, "--repository");
  const scope_id = value(rest, "--scope");
  if (!root || !repository_id || !scope_id) throw new Error(usage());
  const canonical = await canonicalRoot(root);
  const config = value(rest, "--config");
  const native = value(rest, "--native");
  const provider_config = {
    transport: "loopback",
    ...(config ? { config: { path: config } } : {}),
    ...(native ? { native: { name: native } } : {}),
  };
  const receipt = { action: "enroll", root: canonical, repository_id, scope_id, registry: defaultRegistryPath(), dry_run: dryRunFor(rest) };
  if (config || native) receipt.installation = installationFor({ provider_config });
  if (!receipt.dry_run) Object.assign(receipt, await enroll(canonical, { repository_id, scope_id, provider_config, grant_policy: { level: "read-only" } }));
  return receipt;
}

async function uninstall(root, rest) {
  if (!root) throw new Error(usage());
  const binding = await bindingFor(root);
  const revoked_token_generations = [...new Set([...(binding.token_grant?.revoked_generations || []), ...(binding.token_grant ? [binding.token_grant.generation] : [])])].sort((a, b) => a - b);
  const receipt = { action: "uninstall", root: binding.root, repository_id: binding.repository_id, registry: defaultRegistryPath(), revoked_token_generations, dry_run: dryRunFor(rest) };
  if (!receipt.dry_run) Object.assign(receipt, await removeBinding(root));
  return receipt;
}

async function token(subcommand, root, rest) {
  if (!root || !["rotate", "recover"].includes(subcommand)) throw new Error(usage());
  const binding = await bindingFor(root);
  const reason = subcommand === "recover" ? "leak_recovery" : "rotation";
  if (subcommand === "recover" && value(rest, "--reason") !== "leak") throw new Error("token recovery requires --reason leak");
  if (dryRunFor(rest)) {
    return {
      action: "token_rotate", root: binding.root, repository_id: binding.repository_id, registry: defaultRegistryPath(),
      token_generation: (binding.token_grant?.generation || 0) + 1,
      revoked_token_generations: [...new Set([...(binding.token_grant?.revoked_generations || []), ...(binding.token_grant ? [binding.token_grant.generation] : [])])].sort((a, b) => a - b),
      reason, dry_run: true,
    };
  }
  return { action: "token_rotate", registry: defaultRegistryPath(), dry_run: false, ...(await rotateToken(root, { reason })) };
}

if (command === "init") {
  process.stdout.write(JSON.stringify(await init(args[0], args.slice(1))) + "\n");
} else if (command === "uninstall") {
  process.stdout.write(JSON.stringify(await uninstall(args[0], args.slice(1))) + "\n");
} else if (command === "token") {
  process.stdout.write(JSON.stringify(await token(args[0], args[1], args.slice(2))) + "\n");
} else {
  throw new Error(usage());
}
