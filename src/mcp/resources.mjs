// D31: read-only MCP resources — manifest, language/provider matrix,
// architecture, claims/conflicts, rule results, receipts. URI-addressable,
// paginated, root-scoped.

import { manifestDigest, languageCapabilityRecords } from "../graph/language-registry.mjs";

export const RESOURCE_URIS = Object.freeze([
  "cortex://manifest",
  "cortex://languages",
  "cortex://providers",
  "cortex://architecture",
  "cortex://claims",
  "cortex://conflicts",
  "cortex://rules",
  "cortex://receipts",
]);

export function resourceForUri(uri, { service, repoId, limit = 50, cursor = null } = {}) {
  switch (uri) {
    case "cortex://manifest":
      return { uri, schemaVersion: 1, digest: manifestDigest(), languages: languageCapabilityRecords().length };
    case "cortex://languages":
      return { uri, schemaVersion: 1, languages: languageCapabilityRecords() };
    case "cortex://providers":
      return { uri, schemaVersion: 1, providers: [{ id: "cortex-static", precisionTier: "LEXICAL" }, { id: "cortex-treesitter", precisionTier: "AST" }] };
    case "cortex://claims":
      return { uri, schemaVersion: 1, claims: [], pagination: { limit, cursor, nextCursor: null } };
    case "cortex://conflicts":
      return { uri, schemaVersion: 1, conflicts: [], pagination: { limit, cursor, nextCursor: null } };
    case "cortex://rules":
      return { uri, schemaVersion: 1, rules: [], pagination: { limit, cursor, nextCursor: null } };
    case "cortex://receipts":
      return { uri, schemaVersion: 1, receipts: [], pagination: { limit, cursor, nextCursor: null } };
    case "cortex://architecture":
      return { uri, schemaVersion: 1, architecture: null, pagination: { limit, cursor, nextCursor: null } };
    default:
      return { uri, error: { code: "resource_not_found", message: `unknown resource ${uri}` } };
  }
}
