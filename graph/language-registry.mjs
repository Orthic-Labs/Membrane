// D20: language registry — one source of truth for language capability
// records, extension routing, and manifest digest. Exposes `cortex languages
// --json` data and retains lexical fallback for unknown extensions.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CATALOG = JSON.parse(readFileSync(join(ROOT, "grammars", "catalog.json"), "utf8"));
const MANIFEST_PATH = join(ROOT, "grammars", "manifest.json");

let manifestCache = null;
function readManifest() {
  if (manifestCache) return manifestCache;
  manifestCache = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
  return manifestCache;
}

export function manifestDigest() {
  return readManifest().digest ?? null;
}

export function languageByExtension(extension) {
  const ext = String(extension).replace(/^\./, "").toLowerCase();
  const found = CATALOG.grammars.find((grammar) => grammar.extensions.includes(ext));
  if (found) return { ...found, precisionTier: found.precisionTier, fallback: false };
  return { language: ext, extensions: [ext], factProfile: "unknown", precisionTier: "LEXICAL", fallback: true };
}

export function languageCapabilityRecords() {
  return CATALOG.grammars.map((grammar) => ({
    language: grammar.language,
    extensions: grammar.extensions,
    factProfile: grammar.factProfile,
    precisionTier: grammar.precisionTier,
    grammarFile: grammar.grammarFile,
    limits: grammar.limits ?? [],
  }));
}

export function languagesJson() {
  return {
    schemaVersion: 1,
    digest: manifestDigest(),
    languages: languageCapabilityRecords(),
  };
}
