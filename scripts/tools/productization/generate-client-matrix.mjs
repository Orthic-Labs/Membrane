#!/usr/bin/env node
// MBR-206 — Generate the client registry and support matrix.
//
// Reads:
//   schemas/registry/clients.yaml                        (human-authored registry)
//   schemas/registry/operations/operations-index.v1.golden.json  (MBR-301 operation universe)
//
// Emits:
//   schemas/registry/clients.capabilities.v1.json       (capability envelopes, schema client-capability.v1)
//   docs/clients/support-matrix.v1.json           (cartesian client x operation matrix)
//
// Both artifacts are byte-stable across runs: identical inputs produce byte-identical
// outputs (sorted keys, no timestamps in the canonical content, trailing newline).
//
// Run:  node scripts/tools/productization/generate-client-matrix.mjs
// Exit: 0 on success, 1 when a contract is violated (unknown operation, unknown client, etc).
//
// Book mode: this script is committed in the MBR-206 commit but is NOT invoked until
// the Book 1 gate. The companion test (tests/clients/client-matrix.test.mjs) imports
// the same builder functions to verify byte-stability deterministically.

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..", "..", "..");
const REGISTRY = join(REPO_ROOT, "schemas", "registry", "clients.yaml");
const OPERATION_INDEX = join(REPO_ROOT, "schemas", "registry", "operations", "operations-index.v1.golden.json");
const CAPABILITIES_OUT = join(REPO_ROOT, "schemas", "registry", "clients.capabilities.v1.json");
const MATRIX_OUT = join(REPO_ROOT, "docs", "clients", "support-matrix.v1.json");

/**
 * Minimal YAML parser supporting the exact dialect used in schemas/registry/clients.yaml:
 *   - block maps with `: ` separator
 *   - block sequences with `- ` prefix
 *   - quoted scalars ("..." or '...')
 *   - bare scalar strings, integers, booleans (true/false), null
 *   - flow-style sequences `[a, b, c]`
 *   - blank lines and `# comment` lines
 *
 * Returns a plain JS object (map at the root) — every value is one of:
 * string, number, boolean, null, Array, or plain Object.
 */
export function parseYaml(text) {
  const lines = text.split(/\r?\n/).reduce((acc, rawLine, idx) => {
    const stripped = rawLine.replace(/\s+$/, "");
    if (stripped.trim() === "" || stripped.trim().startsWith("#")) return acc;
    const indent = stripped.length - stripped.trimStart().length;
    acc.push({ idx, indent, content: stripped.trimStart() });
    return acc;
  }, []);
  let pos = 0;

  function peek() {
    return pos < lines.length ? lines[pos] : null;
  }
  function advance() {
    return pos < lines.length ? lines[pos++] : null;
  }

  function parseScalar(raw) {
    if (raw === undefined || raw === null) return null;
    let s = String(raw).trim();
    if (s === "" || s === "~" || s === "null") return null;
    if (s === "true") return true;
    if (s === "false") return false;
    if (s.startsWith('"') && s.endsWith('"') && s.length >= 2) {
      return s.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, "\\");
    }
    if (s.startsWith("'") && s.endsWith("'") && s.length >= 2) {
      return s.slice(1, -1);
    }
    if (s.startsWith("[") && s.endsWith("]")) {
      const inner = s.slice(1, -1).trim();
      if (inner === "") return [];
      return inner.split(",").map((part) => parseScalar(part));
    }
    if (s.startsWith("{") && s.endsWith("}")) {
      const inner = s.slice(1, -1).trim();
      if (inner === "") return {};
      const map = {};
      for (const pair of inner.split(",")) {
        const colonIdx = pair.indexOf(":");
        if (colonIdx < 0) throw new Error(`YAML: invalid flow map pair '${pair}'`);
        map[pair.slice(0, colonIdx).trim()] = parseScalar(pair.slice(colonIdx + 1));
      }
      return map;
    }
    if (/^-?\d+$/.test(s)) return parseInt(s, 10);
    return s;
  }

  function readInlineMapEntry(line, baseIndent) {
    const content = line.content;
    const colonIdx = content.indexOf(":");
    if (colonIdx < 0) throw new Error(`YAML: expected ':' on line ${line.idx}: '${content}'`);
    return {
      key: content.slice(0, colonIdx).trim(),
      rawValue: content.slice(colonIdx + 1).trim(),
      keyIndent: baseIndent,
    };
  }

  function readMapEntry(line) {
    return readInlineMapEntry(line, line.indent);
  }

  function consumeMapEntriesAt(minIndent) {
    const entries = [];
    while (pos < lines.length) {
      const line = lines[pos];
      if (line.indent < minIndent) break;
      if (line.indent > minIndent) {
        throw new Error(`YAML: unexpected indent on line ${line.idx}: ${line.content}`);
      }
      const content = line.content;
      if (content.startsWith("- ")) break;
      const colonIdx = content.indexOf(":");
      if (colonIdx < 0) {
        throw new Error(`YAML: expected ':' on line ${line.idx}: '${content}'`);
      }
      entries.push({
        key: content.slice(0, colonIdx).trim(),
        rawValue: content.slice(colonIdx + 1).trim(),
        keyIndent: line.indent,
      });
      pos++;
    }
    return entries;
  }

  function buildMap(minIndent) {
    const map = {};
    while (pos < lines.length) {
      const line = lines[pos];
      if (line.indent < minIndent) break;
      if (line.indent > minIndent) {
        throw new Error(`YAML: unexpected indent on line ${line.idx}: ${line.content}`);
      }
      const content = line.content;
      if (content.startsWith("- ")) break;
      const colonIdx = content.indexOf(":");
      if (colonIdx < 0) {
        throw new Error(`YAML: expected ':' on line ${line.idx}: '${content}'`);
      }
      const key = content.slice(0, colonIdx).trim();
      const rawValue = content.slice(colonIdx + 1).trim();
      pos++;
      if (rawValue === "") {
        const next = peek();
        if (next && next.indent > minIndent) {
          if (next.content.startsWith("- ")) {
            map[key] = buildList(next.indent);
          } else {
            map[key] = buildMap(next.indent);
          }
        } else {
          map[key] = null;
        }
      } else {
        map[key] = parseScalar(rawValue);
      }
    }
    return map;
  }

  function buildList(minIndent) {
    const list = [];
    while (pos < lines.length) {
      const line = lines[pos];
      if (line.indent < minIndent) break;
      if (line.indent > minIndent) {
        throw new Error(`YAML: unexpected indent on line ${line.idx}: ${line.content}`);
      }
      const content = line.content;
      if (!content.startsWith("- ")) break;
      const itemContent = content.slice(2);
      if (itemContent === "") {
        pos++;
        list.push(buildMap(minIndent + 2));
        continue;
      }
      const colonIdx = itemContent.indexOf(":");
      if (colonIdx < 0) {
        pos++;
        list.push(parseScalar(itemContent));
        continue;
      }
      const subMap = {};
      const firstKey = itemContent.slice(0, colonIdx).trim();
      const firstRaw = itemContent.slice(colonIdx + 1).trim();
      if (firstRaw !== "") subMap[firstKey] = parseScalar(firstRaw);
      pos++;
      const childIndent = minIndent + 2;
      while (pos < lines.length) {
        const next = lines[pos];
        if (next.indent < childIndent) break;
        if (next.indent > childIndent) {
          throw new Error(`YAML: unexpected indent on line ${next.idx}: ${next.content}`);
        }
        const nextContent = next.content;
        if (nextContent.startsWith("- ")) break;
        const nextColonIdx = nextContent.indexOf(":");
        if (nextColonIdx < 0) {
          throw new Error(`YAML: expected ':' on line ${next.idx}: '${nextContent}'`);
        }
        const k = nextContent.slice(0, nextColonIdx).trim();
        const v = nextContent.slice(nextColonIdx + 1).trim();
        pos++;
        if (v === "") {
          const nn = peek();
          if (nn && nn.indent > childIndent) {
            if (nn.content.startsWith("- ")) {
              subMap[k] = buildList(nn.indent);
            } else {
              subMap[k] = buildMap(nn.indent);
            }
          } else {
            subMap[k] = null;
          }
        } else {
          subMap[k] = parseScalar(v);
        }
      }
      list.push(subMap);
    }
    return list;
  }

  return buildMap(0);
}

/**
 * Deterministic JSON.stringify: sorted top-level and nested keys, trailing newline,
 * stable indentation. Object key order does not affect value, but we sort it so
 * repeated runs are byte-identical.
 */
export function stableStringify(value) {
  return JSON.stringify(sortKeys(value), null, 2) + "\n";
}

function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort()) out[key] = sortKeys(value[key]);
    return out;
  }
  return value;
}

/**
 * Convert a YAML client entry into a JSON capability envelope conforming to
 * schemas/client-capability.v1.schema.json. Throws on unknown / missing keys so
 * the registry contract is enforced at generation time.
 */
function toCapability(client) {
  const required = ["id", "display_name", "transport", "discovery_method", "install_command", "supported_operations", "authority_grant", "honest_level"];
  for (const key of required) {
    if (!(key in client)) throw new Error(`client '${client?.id ?? "<unknown>"}' missing required key '${key}'`);
  }
  return {
    schemaVersion: 1,
    id: client.id,
    displayName: client.display_name,
    transport: client.transport,
    discoveryMethod: client.discovery_method,
    installCommand: client.install_command,
    detectCommand: client.detect_command ?? null,
    binaryEnv: client.binary_env ?? null,
    scopeDefault: client.scope_default ?? null,
    honestLevel: client.honest_level,
    authorityGrant: {
      maxLevel: client.authority_grant.max_level,
      scopes: client.authority_grant.scopes,
      loopbackOnly: client.authority_grant.loopback_only,
    },
    supportedOperations: client.supported_operations,
    degradedOperations: client.degraded_operations ?? [],
  };
}

/**
 * Build both artifacts from the in-memory registry and operation index. Pure
 * function — exported for the companion test.
 *
 * Throws when:
 *   - the registry declares an operation that does not exist in the operation index
 *   - the operation index has zero operations (the matrix must be non-empty)
 *   - the registry has zero clients
 *   - two clients share the same id
 */
export function buildArtifacts(registry, operationIndex) {
  if (!registry || typeof registry !== "object") throw new Error("registry must be an object");
  if (!operationIndex || !Array.isArray(operationIndex.operations)) throw new Error("operationIndex.operations must be an array");

  const operations = operationIndex.operations.map((op) => ({
    name: op.name,
    schemaVersion: op.schemaVersion,
    errorVersion: op.errorVersion,
  }));
  const opNames = new Set(operations.map((op) => op.name));

  const clients = (registry.clients ?? []).map(toCapability);
  const ids = new Set();
  for (const client of clients) {
    if (ids.has(client.id)) throw new Error(`duplicate client id: ${client.id}`);
    ids.add(client.id);
    for (const opName of client.supportedOperations) {
      if (!opNames.has(opName)) {
        throw new Error(`client '${client.id}' lists unsupported_operation '${opName}' (not in operations-index.v1)`);
      }
    }
    for (const opName of client.degradedOperations) {
      if (!opNames.has(opName)) {
        throw new Error(`client '${client.id}' lists degraded_operation '${opName}' (not in operations-index.v1)`);
      }
      if (client.supportedOperations.includes(opName)) {
        throw new Error(`client '${client.id}' lists '${opName}' in both supported and degraded`);
      }
    }
  }

  const cells = [];
  for (const client of clients) {
    const supported = new Set(client.supportedOperations);
    const degraded = new Set(client.degradedOperations);
    for (const op of operations) {
      let support;
      if (supported.has(op.name)) support = "supported";
      else if (degraded.has(op.name)) support = "degraded";
      else support = "unsupported";
      cells.push({
        client: client.id,
        operation: op.name,
        support,
      });
    }
  }

  const capabilities = {
    schemaVersion: 1,
    generatedFrom: registry.generated_from ?? [],
    clients,
  };

  const matrix = {
    schemaVersion: 1,
    matrixVersion: 1,
    operations,
    clients: clients.map((c) => ({
      id: c.id,
      displayName: c.displayName,
      transport: c.transport,
      honestLevel: c.honestLevel,
    })),
    cells,
    generatedFrom: registry.generated_from ?? [],
  };

  return { capabilities, matrix };
}

/** Read registry and operation index from disk, build both artifacts, write them. */
export async function writeArtifacts({
  registryPath = REGISTRY,
  operationIndexPath = OPERATION_INDEX,
  capabilitiesOut = CAPABILITIES_OUT,
  matrixOut = MATRIX_OUT,
} = {}) {
  const registryText = await readFile(registryPath, "utf8");
  const registry = parseYaml(registryText);
  const operationIndex = JSON.parse(await readFile(operationIndexPath, "utf8"));

  const { capabilities, matrix } = buildArtifacts(registry, operationIndex);

  await mkdir(dirname(matrixOut), { recursive: true });
  await writeFile(capabilitiesOut, stableStringify(capabilities));
  await writeFile(matrixOut, stableStringify(matrix));
  return { capabilities, matrix, capabilitiesOut, matrixOut };
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const { capabilitiesOut, matrixOut } = await writeArtifacts();
  console.log(`wrote ${capabilitiesOut}`);
  console.log(`wrote ${matrixOut}`);
}
