// BPT-057: refuse poisoned manifests or plugins BEFORE Blueprint acceptance.
//
// "Before acceptance" is the whole contract. Admission here never imports,
// evaluates or spawns anything the manifest names: it reads bytes and decides.
// A plugin only becomes a candidate for execution after every gate below has
// passed, and execution itself still goes through `runProvider`'s isolation.
//
// Every refusal is typed and recorded. A manifest that cannot be admitted
// disappears from no accounting — the caller receives a refusal disposition
// naming the gate that rejected it, so a poisoned plugin is observable rather
// than silently skipped.

import { createHash } from "node:crypto";
import { readFileSync, readdirSync, realpathSync, statSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";

import { definePlugin, PLUGIN_TYPES } from "../sdk/providers.mjs";
import { validateProviderManifest } from "./index.mjs";

/** Repository-relative directory a repository may ship plugin manifests in. */
export const PLUGIN_MANIFEST_DIR = ".agent/plugins";

/** Terminal admission dispositions. Every considered manifest gets exactly one. */
export const PLUGIN_DISPOSITIONS = Object.freeze(["admitted", "refused"]);

function refusal(source, code, reason, detail = {}) {
  return Object.freeze({ source, disposition: "refused", code, reason, ...detail });
}

/**
 * Resolve `candidate` and prove it stays inside `root` even after symlinks.
 * Returns null when it escapes, so callers refuse rather than read.
 */
function confine(root, candidate) {
  const resolved = resolve(root, candidate);
  const rel = relative(root, resolved);
  if (rel === "" || rel.startsWith("..") || rel.split(sep).includes("..")) return null;
  let real;
  try {
    real = realpathSync(resolved);
  } catch {
    // A path that does not exist cannot escape by symlink; the read below is
    // what will fail, and it fails as a typed refusal rather than an escape.
    return resolved;
  }
  let realRoot;
  try {
    realRoot = realpathSync(root);
  } catch {
    return null;
  }
  const realRel = relative(realRoot, real);
  if (realRel.startsWith("..") || realRel.split(sep).includes("..")) return null;
  return real;
}

/**
 * List candidate manifest files. Discovery itself is confined: a symlinked
 * plugin directory pointing outside the repository yields nothing.
 */
export function discoverPluginManifests(repoRoot, { dir = PLUGIN_MANIFEST_DIR } = {}) {
  const root = resolve(repoRoot);
  const pluginDir = confine(root, dir);
  if (!pluginDir) return [];
  let entries;
  try {
    if (!statSync(pluginDir).isDirectory()) return [];
    entries = readdirSync(pluginDir);
  } catch {
    return [];
  }
  return entries
    .filter((name) => name.toLowerCase().endsWith(".json"))
    .sort()
    .map((name) => join(dir, name));
}

/**
 * The acceptance gate. `manifestPath` is repository-relative.
 *
 * Gates, in order, each one refusing before the next reads anything wider:
 *  1. path confinement of the manifest itself;
 *  2. readable, parseable JSON;
 *  3. declared plugin type is one Blueprint knows;
 *  4. provider-manifest shape: id/version/license/integrity/entry present,
 *     entry repository-relative (no absolute, drive-letter, `..` or URL),
 *     integrity well-formed, licence inside the allowlist;
 *  5. permission ceiling — a manifest that *asks for* more than repo-read /
 *     no-network / no-process is refused, never silently downgraded;
 *  6. the artifact the entry names is confined and its bytes hash to the
 *     declared integrity;
 *  7. the publisher is trusted, when a trust list is configured.
 *
 * Nothing the manifest names is imported or executed at any point.
 */
export function admitPluginManifest(repoRoot, manifestPath, {
  allowedLicenses = null,
  trustedPublishers = null,
} = {}) {
  const root = resolve(repoRoot);
  const source = String(manifestPath).replaceAll("\\", "/");

  const confined = confine(root, manifestPath);
  if (!confined) return refusal(source, "plugin_manifest_path_escapes_repository", "plugin manifest path escapes the repository root");

  let raw;
  try {
    raw = readFileSync(confined, "utf8");
  } catch (error) {
    return refusal(source, "plugin_manifest_unreadable", `plugin manifest could not be read: ${error?.message ?? error}`);
  }

  let manifest;
  try {
    manifest = JSON.parse(raw);
  } catch (error) {
    return refusal(source, "plugin_manifest_malformed", `plugin manifest is not valid JSON: ${error?.message ?? error}`);
  }
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    return refusal(source, "plugin_manifest_malformed", "plugin manifest must be a JSON object");
  }
  if (!PLUGIN_TYPES.includes(String(manifest.type))) {
    return refusal(source, "plugin_type_unknown", `unknown plugin type ${manifest.type}`);
  }

  let validated;
  try {
    validated = validateProviderManifest(manifest, { allowedLicenses });
  } catch (error) {
    return refusal(source, error?.code ?? "plugin_manifest_invalid", error?.message ?? String(error));
  }

  // The permission ceiling is checked against the DECLARED manifest, before
  // the artifact is read: a manifest that asks for escalation is refused even
  // if its bytes would have hashed correctly.
  let plugin;
  try {
    plugin = definePlugin({
      id: validated.id,
      version: validated.version,
      protocolRange: validated.protocolRange ?? ">=1 <2",
      type: validated.type,
      capabilities: validated.capabilities ?? [],
      permissions: validated.permissions ?? {},
      hash: validated.integrity,
    });
  } catch (error) {
    return refusal(source, "plugin_permission_refused", error?.message ?? String(error));
  }

  const entryPath = confine(root, validated.entry);
  if (!entryPath) return refusal(source, "plugin_entry_escapes_repository", `plugin entry escapes the repository root: ${validated.entry}`);
  let bytes;
  try {
    bytes = readFileSync(entryPath);
  } catch (error) {
    return refusal(source, "plugin_artifact_unreadable", `plugin artifact could not be read: ${error?.message ?? error}`);
  }
  const observed = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
  if (observed !== validated.integrity.toLowerCase()) {
    return refusal(source, "plugin_integrity_mismatch", "plugin artifact checksum does not match the manifest", {
      expected: validated.integrity,
      observed,
    });
  }

  if (Array.isArray(trustedPublishers) && trustedPublishers.length) {
    const publisher = validated.publisher === undefined ? null : String(validated.publisher);
    if (!publisher || !trustedPublishers.includes(publisher)) {
      return refusal(source, "plugin_publisher_untrusted", `plugin publisher ${publisher ?? "<absent>"} is not trusted`, { publisher });
    }
  }

  return Object.freeze({
    source,
    disposition: "admitted",
    id: plugin.id,
    version: plugin.version,
    type: validated.type,
    entry: validated.entry,
    integrity: validated.integrity,
    license: validated.license,
    permissions: plugin.permissions,
    capabilities: plugin.capabilities,
  });
}

/**
 * Admit every manifest a repository ships. Returns a receipt, not a side
 * effect: no plugin code is loaded here. `refused` is never empty-by-omission
 * — a manifest that failed any gate appears in it with the gate's code.
 */
export function admitRepositoryPlugins(repoRoot, options = {}) {
  const sources = discoverPluginManifests(repoRoot, options);
  const results = sources.map((source) => admitPluginManifest(repoRoot, source, options));
  return Object.freeze({
    schemaVersion: 1,
    considered: sources.length,
    admitted: Object.freeze(results.filter((row) => row.disposition === "admitted")),
    refused: Object.freeze(results.filter((row) => row.disposition === "refused")),
  });
}
