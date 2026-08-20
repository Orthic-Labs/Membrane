// D31: read-only MCP resources — manifest, language/provider matrix,
// architecture, claims/conflicts, rule results, receipts. URI-addressable,
// paginated, root-scoped.

import { manifestDigest, languageCapabilityRecords } from "../graph/language-registry.mjs";

export const RESOURCE_URIS = Object.freeze([
  "blueprint://manifest",
  "blueprint://languages",
  "blueprint://providers",
  "blueprint://architecture",
  "blueprint://claims",
  "blueprint://conflicts",
  "blueprint://rules",
  "blueprint://receipts",
]);

export function resourceForUri(uri, { service, repoId, limit = 50, cursor = null } = {}) {
  switch (uri) {
    case "blueprint://manifest":
      return { uri, schemaVersion: 1, digest: manifestDigest(), languages: languageCapabilityRecords().length };
    case "blueprint://languages":
      return { uri, schemaVersion: 1, languages: languageCapabilityRecords() };
    case "blueprint://providers":
      return { uri, schemaVersion: 1, providers: [{ id: "blueprint-static", precisionTier: "LEXICAL" }, { id: "blueprint-treesitter", precisionTier: "AST" }] };
    case "blueprint://claims":
      return { uri, schemaVersion: 1, claims: [], pagination: { limit, cursor, nextCursor: null } };
    case "blueprint://conflicts":
      return { uri, schemaVersion: 1, conflicts: [], pagination: { limit, cursor, nextCursor: null } };
    case "blueprint://rules":
      return { uri, schemaVersion: 1, rules: [], pagination: { limit, cursor, nextCursor: null } };
    case "blueprint://receipts":
      return { uri, schemaVersion: 1, receipts: [], pagination: { limit, cursor, nextCursor: null } };
    case "blueprint://architecture":
      return { uri, schemaVersion: 1, architecture: null, pagination: { limit, cursor, nextCursor: null } };
    default:
      return { uri, error: { code: "resource_not_found", message: `unknown resource ${uri}` } };
  }
}
