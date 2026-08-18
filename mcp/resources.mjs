// Canonical Membrane MCP resources — legacy (JS) surface.
//
// Every resource definition lives in `schemas/registry/resources/*.json` so the
// native (Rust) MCP server and this legacy (JS) MCP server serve the exact
// same payload. This module reads the canonical JSON at runtime, exposes the
// same `listResources()` / `readResource(uri, grants)` shape the Rust module
// emits, and returns Membrane-specific metadata (`accessGrants`,
// `authorityEscalation`, `authorityNotes`) so a client can prove the
// resource is bounded before issuing a read.
//
// Read enforcement mirrors the native surface: a read whose presented
// `accessGrants` does not include any entry from the resource's declared
// `accessGrants` returns a typed `resource_access_denied` rejection. The
// resource body is **never** serialized into the response when the read is
// denied, so a denied request cannot leak content across grants.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..");

const RESOURCE_FILES = [
  "resources-index.v1.json",
  "installation-manifest.v1.json",
  "lease-status.v1.json",
  "operation-registry.v1.json",
];

let cached = null;

async function loadAll() {
  if (cached) return cached;
  const entries = await Promise.all(
    RESOURCE_FILES.map(async (file) => {
      const raw = await readFile(join(REPO_ROOT, "schemas", "registry", "resources", file), "utf8");
      return JSON.parse(raw);
    }),
  );
  cached = entries;
  return cached;
}

function shapeListEntry(resource) {
  return {
    name: resource.name,
    version: resource.version,
    uri: resource.uri,
    mimeType: resource.mimeType,
    description: resource.description,
    accessGrants: resource.accessGrants ?? [],
    authorityEscalation: resource.authorityEscalation === true,
    authorityNotes: resource.authorityNotes ?? null,
  };
}

function shapeReadBody(resource) {
  return {
    name: resource.name,
    version: resource.version,
    uri: resource.uri,
    mimeType: resource.mimeType,
    description: resource.description,
    accessGrants: resource.accessGrants ?? [],
    authorityEscalation: resource.authorityEscalation === true,
    authorityNotes: resource.authorityNotes ?? null,
    body: resource.body ?? null,
  };
}

export const RESOURCE_NAMES = ["resources-index", "installation-manifest", "lease-status", "operation-registry"];
export const RESOURCE_URIS = [
  "membrane://resource/resources-index/v1",
  "membrane://resource/installation-manifest/v1",
  "membrane://resource/lease-status/v1",
  "membrane://resource/operation-registry/v1",
];

export async function listResources() {
  const entries = await loadAll();
  return { resources: entries.map((resource) => shapeListEntry(resource)) };
}

export async function allResourceDefinitions() {
  return loadAll();
}

function denialPayload({ resource, required, presented }) {
  return {
    error: {
      kind: "error",
      code: "resource_access_denied",
      message: `resources/read requires one of the grants in accessGrants: [${required.join(", ")}]; caller presented [${presented.join(", ")}]`,
      retryable: false,
      details: {
        resource,
        requiredGrants: required,
        presentedGrants: presented,
      },
    },
  };
}

function notFoundPayload(uri) {
  return {
    error: {
      kind: "error",
      code: "resource_not_found",
      message: `resources/read received an unknown URI: ${uri}`,
      retryable: false,
      details: { uri },
    },
  };
}

export async function readResource(uri, accessGrants = []) {
  const entries = await loadAll();
  const resource = entries.find((entry) => entry.uri === uri);
  if (!resource) return { ok: false, result: notFoundPayload(uri) };
  const required = Array.isArray(resource.accessGrants) ? resource.accessGrants.slice() : [];
  const presented = Array.isArray(accessGrants) ? accessGrants.slice() : [];
  const granted = required.some((grant) => presented.includes(grant));
  if (!granted) {
    return {
      ok: false,
      result: denialPayload({ resource: resource.name, required, presented }),
    };
  }
  return { ok: true, result: shapeReadBody(resource) };
}

export async function readResourceByName(name, accessGrants = []) {
  const entries = await loadAll();
  const resource = entries.find((entry) => entry.name === name);
  if (!resource) return { ok: false, result: notFoundPayload(name) };
  return readResource(resource.uri, accessGrants);
}
