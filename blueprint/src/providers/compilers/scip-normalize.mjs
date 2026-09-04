// Canonical SCIP semantic normalization shared by every first-party SCIP lane.
//
// This module is deliberately policy-neutral: it parses the portable SCIP JSON
// transport, normalizes occurrence roles/ranges, preserves symbol metadata and
// relationships, and builds exact symbol-definition identity. It does NOT assign
// Blueprint confidence, choose graph edge kinds, or perform name matching.

import { readFileSync } from "node:fs";

export const SCIP_ROLE_BITS = Object.freeze({
  definition: 1,
  reference: 2,
  read: 4,
  write: 8,
});

export function normalizeScipRoles(roles) {
  const names = new Set();
  if (Array.isArray(roles)) {
    for (const role of roles) {
      const normalized = String(role).trim().toLowerCase();
      if (normalized) names.add(normalized);
    }
    return names;
  }
  const value = Number(roles);
  if (!Number.isFinite(value)) return names;
  for (const [name, bit] of Object.entries(SCIP_ROLE_BITS)) {
    if ((value & bit) === bit) names.add(name);
  }
  return names;
}

function normalizeRange(range) {
  if (!Array.isArray(range) || range.length < 2) return null;
  const values = range.map((value) => Number(value));
  if (values.some((value) => !Number.isFinite(value))) return null;
  return values;
}

function normalizeRelationships(relationships) {
  if (!Array.isArray(relationships)) return [];
  return relationships
    .filter((relationship) => relationship && typeof relationship === "object" && typeof relationship.symbol === "string" && relationship.symbol.length > 0)
    .map((relationship) => Object.freeze({
      symbol: relationship.symbol,
      isReference: Boolean(relationship.isReference ?? relationship.is_reference),
      isImplementation: Boolean(relationship.isImplementation ?? relationship.is_implementation),
      isTypeDefinition: Boolean(relationship.isTypeDefinition ?? relationship.is_type_definition),
    }));
}

function normalizeSymbolInformation(symbols) {
  if (!Array.isArray(symbols)) return [];
  return symbols
    .filter((info) => info && typeof info === "object" && typeof info.symbol === "string" && info.symbol.length > 0)
    .map((info) => Object.freeze({
      symbol: info.symbol,
      documentation: Array.isArray(info.documentation) ? [...info.documentation] : [],
      relationships: normalizeRelationships(info.relationships),
      kind: info.kind ?? null,
      displayName: info.displayName ?? info.display_name ?? null,
      signatureDocumentation: info.signatureDocumentation ?? info.signature_documentation ?? null,
      enclosingSymbol: info.enclosingSymbol ?? info.enclosing_symbol ?? null,
      raw: info,
    }));
}

function normalizeOccurrence(occurrence, documentPath) {
  if (!occurrence || typeof occurrence !== "object") return null;
  if (typeof occurrence.symbol !== "string" || occurrence.symbol.length === 0) return null;
  const range = normalizeRange(occurrence.range);
  if (!range) return null;
  const roles = normalizeScipRoles(occurrence.roles ?? occurrence.symbolRoles ?? occurrence.symbol_roles);
  if (!roles.size) return null;
  return Object.freeze({
    symbol: occurrence.symbol,
    roles,
    range,
    documentPath,
    overrideDocumentation: Array.isArray(occurrence.overrideDocumentation ?? occurrence.override_documentation)
      ? [...(occurrence.overrideDocumentation ?? occurrence.override_documentation)]
      : [],
    syntaxKind: occurrence.syntaxKind ?? occurrence.syntax_kind ?? null,
    diagnostics: Array.isArray(occurrence.diagnostics) ? [...occurrence.diagnostics] : [],
    rawRoles: occurrence.roles ?? occurrence.symbolRoles ?? occurrence.symbol_roles ?? null,
    raw: occurrence,
  });
}

export function normalizeScipIndex(parsed, { indexPath = null } = {}) {
  if (!parsed || typeof parsed !== "object" || !Array.isArray(parsed.documents)) {
    const error = new Error(`${indexPath ? `SCIP index at ${indexPath}` : "SCIP index"} has no \"documents\" array — not a recognized portable-SCIP-JSON shape`);
    error.code = "scip_index_incompatible";
    throw error;
  }

  const documents = [];
  const definitionsBySymbol = new Map();
  const occurrences = [];
  const symbolInformationBySymbol = new Map();
  let skippedDocuments = 0;
  let skippedOccurrences = 0;

  for (const rawDocument of parsed.documents) {
    if (!rawDocument || typeof rawDocument !== "object") {
      skippedDocuments += 1;
      continue;
    }
    const path = String(rawDocument.relativePath ?? rawDocument.relative_path ?? rawDocument.path ?? "");
    if (!path) {
      skippedDocuments += 1;
      continue;
    }
    const documentOccurrences = [];
    for (const rawOccurrence of rawDocument.occurrences ?? []) {
      const occurrence = normalizeOccurrence(rawOccurrence, path);
      if (!occurrence) {
        skippedOccurrences += 1;
        continue;
      }
      documentOccurrences.push(occurrence);
      occurrences.push(occurrence);
      if (occurrence.roles.has("definition") && !definitionsBySymbol.has(occurrence.symbol)) {
        definitionsBySymbol.set(occurrence.symbol, occurrence);
      }
    }
    const symbols = normalizeSymbolInformation(rawDocument.symbols ?? rawDocument.symbolInformation ?? rawDocument.symbol_information);
    for (const info of symbols) if (!symbolInformationBySymbol.has(info.symbol)) symbolInformationBySymbol.set(info.symbol, info);
    documents.push(Object.freeze({
      path,
      occurrences: Object.freeze(documentOccurrences),
      symbols: Object.freeze(symbols),
      language: rawDocument.language ?? null,
      raw: rawDocument,
    }));
  }

  const externalSymbols = normalizeSymbolInformation(
    parsed.externalSymbols ?? parsed.external_symbols ?? parsed.symbolInformation ?? parsed.symbol_information,
  );
  for (const info of externalSymbols) if (!symbolInformationBySymbol.has(info.symbol)) symbolInformationBySymbol.set(info.symbol, info);

  return Object.freeze({
    metadata: Object.freeze({ ...(parsed.metadata ?? {}) }),
    documents: Object.freeze(documents),
    occurrences: Object.freeze(occurrences),
    definitionsBySymbol,
    symbolInformationBySymbol,
    externalSymbols: Object.freeze(externalSymbols),
    skippedDocuments,
    skippedOccurrences,
    indexPath,
    raw: parsed,
  });
}

export function readNormalizedScipIndex(indexPath) {
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(indexPath, "utf8"));
  } catch (error) {
    const wrapped = new Error(`SCIP index at ${indexPath} could not be read/parsed as JSON: ${String(error?.message ?? error)}`);
    wrapped.code = "scip_index_unreadable";
    wrapped.cause = error;
    throw wrapped;
  }
  return normalizeScipIndex(parsed, { indexPath });
}

export function scipOccurrenceEvidence(occurrence, extra = {}) {
  const [startLine, startCharacter, endLine = startLine, endCharacter = startCharacter] = occurrence.range;
  return Object.freeze({
    path: occurrence.documentPath,
    startLine: startLine + 1,
    startCharacter,
    endLine: endLine + 1,
    endCharacter,
    symbol: occurrence.symbol,
    ...extra,
  });
}
