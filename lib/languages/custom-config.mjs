// D26: custom language configuration — cortex.languages.toml entries point
// to repo-confined WASM, table/query file, hash, extensions, and capability
// profile. Absolute paths, parent traversal, URLs, and built-in IDs/extensions
// are rejected. Built-in language IDs and extensions cannot be overridden.

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve, isAbsolute, relative } from "node:path";

import { languageCapabilityRecords } from "../../graph/language-registry.mjs";

export const CUSTOM_CONFIG_VERSION = 1;

function parseToml(source) {
  // Minimal deterministic TOML subset for cortex.languages.toml (tables of
  // scalars + arrays). The format is pinned; a full TOML parser is not needed.
  const root = {};
  let current = root;
  for (const rawLine of String(source).split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const header = line.match(/^\[\[?([a-zA-Z0-9_.-]+)\]?\]?\s*$/);
    if (header) {
      const key = header[1];
      if (!current[key]) current[key] = [];
      current = { __table: current[key] };
      continue;
    }
    const kv = line.match(/^([a-zA-Z0-9_.-]+)\s*=\s*(.+)$/);
    if (!kv) continue;
    const [, key, rawValue] = kv;
    let value;
    if (rawValue.startsWith('"')) value = rawValue.slice(1, rawValue.lastIndexOf('"'));
    else if (rawValue.startsWith("[")) value = rawValue.slice(1, -1).split(",").map((s) => s.trim().replace(/^"|"$/g, "")).filter(Boolean);
    else if (rawValue === "true" || rawValue === "false") value = rawValue === "true";
    else value = Number(rawValue);
    const target = current.__table ? current.__table[current.__table.length - 1] ?? (current.__table.push({}) && current.__table[current.__table.length - 1]) : current;
    target[key] = value;
  }
  return root;
}

function sha256Hex(input) {
  return createHash("sha256").update(input).digest("hex");
}

export function loadCustomLanguages({ root = process.cwd(), configPath = "cortex.languages.toml" } = {}) {
  const fullPath = resolve(root, configPath);
  if (!existsSync(fullPath)) return { version: CUSTOM_CONFIG_VERSION, languages: [], errors: [] };
  let parsed;
  try {
    parsed = parseToml(readFileSync(fullPath, "utf8"));
  } catch (error) {
    return { version: CUSTOM_CONFIG_VERSION, languages: [], errors: [`${configPath} unparseable: ${error.message}`] };
  }
  const builtins = new Set(languageCapabilityRecords().map((r) => r.language));
  const builtinExtensions = new Set(languageCapabilityRecords().flatMap((r) => r.extensions));
  const languages = [];
  const errors = [];
  for (const entry of parsed.languages ?? []) {
    const id = String(entry.id ?? "");
    const extensions = Array.isArray(entry.extensions) ? entry.extensions.map(String) : [];
    const grammar = String(entry.grammar ?? "");
    if (!id) { errors.push("custom language missing id"); continue; }
    if (builtins.has(id)) { errors.push(`language_route_conflict: ${id} is a built-in id`); continue; }
    if (extensions.some((ext) => builtinExtensions.has(ext))) { errors.push(`language_route_conflict: extension in use by a built-in`); continue; }
    if (isAbsolute(grammar) || grammar.includes("..") || /^https?:\/\//.test(grammar)) { errors.push(`path_conflict: ${grammar} must be repo-confined`); continue; }
    const grammarPath = resolve(root, grammar);
    if (!grammarPath.startsWith(resolve(root))) { errors.push(`path_escape: ${grammar}`); continue; }
    if (!existsSync(grammarPath)) { errors.push(`grammar_missing: ${grammar}`); continue; }
    const bytes = readFileSync(grammarPath);
    if (entry.grammar_sha256 && sha256Hex(bytes) !== entry.grammar_sha256) { errors.push(`hash_mismatch: ${grammar}`); continue; }
    languages.push({
      id,
      extensions,
      grammar,
      grammarSha256: sha256Hex(bytes),
      factProfile: entry.fact_profile ?? "code",
      precisionTier: entry.precision_tier ?? "AST",
      table: entry.table ?? null,
      version: CUSTOM_CONFIG_VERSION,
    });
  }
  return { version: CUSTOM_CONFIG_VERSION, languages, errors };
}
