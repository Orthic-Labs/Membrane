#!/usr/bin/env node
// Native contract fixture manifest checker (migration spec N1).
//
// Validates migration/native-rust/fixtures/native-contracts-manifest.v1.json:
//   - every frozen fixture file exists and matches its recorded SHA-256;
//   - the aggregate digest reproduces;
//   - golden examples validate against their schemas via a bounded draft-07
//     subset (type/required/enum/const/pattern/min/max/items/properties);
//   - internal contracts stay internal: none of the three N1-named internal V1
//     contracts appears in the public protocol registry (schemas/*.schema.json);
//   - the five public Membrane V1 shapes remain present in that registry,
//     untouched.
//
// CLI modes:
//   node scripts/ci/check-native-contract-fixtures.mjs           -> check
//   node scripts/ci/check-native-contract-fixtures.mjs --json    -> machine report
//   node scripts/ci/check-native-contract-fixtures.mjs --update [--write]
//       regenerate the manifest (stdout unless --write); use only when freezing
//       a NEW corpus version, never to paper over in-place edits.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const MANIFEST_REL = "migration/native-rust/fixtures/native-contracts-manifest.v1.json";
export const FIXTURES_DIR = "migration/native-rust/fixtures/native-contracts.v1";
export const PUBLIC_REGISTRY_DIR = "schemas";

export const INTERNAL_CONTRACT_NAMES = [
  "TranscriptEventV1",
  "FailureEpisodeV1",
  "InsightIssueV1",
];

export const PUBLIC_V1_SHAPES = [
  "ScopeGrantV1",
  "ContextCandidateSetV1",
  "ContextPacketV1",
  "ContextReceiptV1",
  "KnowledgeEmissionV1",
];

export const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

export function canonicalFixtureBytes(buf) {
  return Buffer.from(buf).toString("utf8").replaceAll("\r\n", "\n");
}

export function fixtureSha256(path) {
  return sha256(canonicalFixtureBytes(readFileSync(path)));
}

export function aggregateDigest(root, files) {
  const lines = [...files].sort().map((f) => `${f}:${fixtureSha256(join(root, f))}\n`);
  return sha256(lines.join(""));
}

export function buildManifest({ root, now = new Date() }) {
  const dir = join(root, FIXTURES_DIR);
  const files = [];
  const walk = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      const full = join(d, e.name);
      if (e.isDirectory()) walk(full);
      else if (e.name.endsWith(".json") || e.name.endsWith(".md")) {
        files.push(full.slice(root.length + 1).split("\\").join("/"));
      }
    }
  };
  walk(dir);
  files.sort();

  const fileEntry = (path, role) => ({
    path,
    role,
    sha256: fixtureSha256(join(root, path)),
  });
  const contract = (name, schemaVersionTag, sourceOfTruth, schemaFile, exampleFile, extra = {}) => ({
    name,
    schemaVersionTag,
    visibility: "internal",
    sourceOfTruth,
    fixtureFiles: [
      fileEntry(schemaFile, "schema"),
      ...(exampleFile ? [fileEntry(exampleFile, "golden-example")] : []),
    ],
    ...extra,
  });

  const manifest = {
    schemaVersion: 1,
    artifact: "membrane.native-contract-fixtures",
    corpusId: "native-contracts-v1",
    immutable: true,
    generatedAt: now.toISOString(),
    visibilityNote:
      "All contracts here are internal Adapt/transcript domain contracts. They are recorded for N1 freeze and native-port differential testing; none is added to the public protocol registry. The five public V1 shapes are unchanged.",
    contracts: [
      contract(
        "TranscriptEventV1", "membrane.transcript-event.v1",
        "engine/crates/membrane-transcript/src/event.rs",
        `${FIXTURES_DIR}/transcript-event-v1.schema.json`,
        `${FIXTURES_DIR}/examples/transcript-event-v1.example.json`,
      ),
      contract(
        "FailureEpisodeV1", "adapt.failure-episode.v1",
        "engine/crates/membrane-adapt/src/insights/mod.rs",
        `${FIXTURES_DIR}/failure-episode-v1.schema.json`,
        `${FIXTURES_DIR}/examples/failure-episode-v1.example.json`,
      ),
      contract(
        "InsightIssueV1", "adapt.insight-issue.v1",
        "engine/crates/membrane-adapt/src/insights/mod.rs",
        `${FIXTURES_DIR}/insight-issue-v1.schema.json`,
        `${FIXTURES_DIR}/examples/insight-issue-v1.example.json`,
      ),
      contract(
        "PreferenceRecordV1", "adapt.preference-record.v1",
        "engine/crates/membrane-adapt/src/record.rs",
        `${FIXTURES_DIR}/preference-record-v1.schema.json`,
        `${FIXTURES_DIR}/examples/preference-record-v1.example.json`,
      ),
      contract(
        "PreferenceManifest", "1.3.0",
        "frozen snapshot of adapt/src/adapt/preference-manifest.schema.json",
        `${FIXTURES_DIR}/frozen-preference-manifest.schema.json`,
        null,
        { note: "Verbatim snapshot of the canonical Python-era schema at freeze time; language-neutral and executable-free." },
      ),
      contract(
        "RemediationProposal", "remediation-proposal.v1",
        "frozen snapshot of adapt/src/adapt/remediation-proposal.schema.json",
        `${FIXTURES_DIR}/frozen-remediation-proposal.schema.json`,
        null,
        { note: "Verbatim snapshot of the canonical Python-era schema at freeze time; referenced by InsightIssueV1.state.mitigation_links." },
      ),
    ],
    aggregateSha256: null,
    intentionalDeltas: [
      {
        id: "no-bug-for-bug-parity",
        note: "Known-wrong legacy behavior (Adapt manifest hashing gaps, fail-open scope normalization, lossy mirrors, retired-rule core compilation, magic root scope) is intentionally NOT frozen as normative; ports implement the corrected semantics per migration spec section 13.5.",
      },
    ],
  };
  manifest.aggregateSha256 = aggregateDigest(root, readdirRecursiveJsonMd(join(root, FIXTURES_DIR), root));
  return manifest;
}

// Bounded draft-07 subset validator — enough for these fixtures; deliberately
// not a general JSON Schema engine.
export function validateAgainstSchema(instance, schema, path = "$") {
  const errors = [];
  const typeOf = (v) =>
    Array.isArray(v) ? "array" :
    v === null ? "null" :
    typeof v === "number" && Number.isInteger(v) ? "integer" :
    typeof v;

  if (schema.const !== undefined && instance !== schema.const) {
    errors.push(`${path}: expected const ${JSON.stringify(schema.const)}, got ${JSON.stringify(instance)}`);
  }
  if (schema.enum && !schema.enum.includes(instance)) {
    errors.push(`${path}: ${JSON.stringify(instance)} not in enum [${schema.enum.join(", ")}]`);
  }
  if (schema.type) {
    const t = Array.isArray(schema.type) ? schema.type : [schema.type];
    const actual = typeOf(instance);
    if (!t.includes(actual) && !(t.includes("number") && actual === "integer")) {
      errors.push(`${path}: expected type ${t.join("|")}, got ${actual}`);
      return errors;
    }
  }
  if (typeof instance === "number") {
    if (schema.minimum !== undefined && instance < schema.minimum) errors.push(`${path}: below minimum ${schema.minimum}`);
    if (schema.maximum !== undefined && instance > schema.maximum) errors.push(`${path}: above maximum ${schema.maximum}`);
  }
  if (typeof instance === "string" && schema.pattern && !new RegExp(schema.pattern).test(instance)) {
    errors.push(`${path}: does not match pattern ${schema.pattern}`);
  }
  if (schema.minLength !== undefined && typeof instance === "string" && instance.length < schema.minLength) {
    errors.push(`${path}: shorter than minLength ${schema.minLength}`);
  }
  if (typeOf(instance) === "object") {
    for (const req of schema.required ?? []) {
      if (!(req in instance)) errors.push(`${path}: missing required property '${req}'`);
    }
    if (schema.additionalProperties === false && schema.properties) {
      for (const key of Object.keys(instance)) {
        if (!(key in schema.properties)) errors.push(`${path}: additional property '${key}' not allowed`);
      }
    }
    for (const [key, sub] of Object.entries(schema.properties ?? {})) {
      if (key in instance) errors.push(...validateAgainstSchema(instance[key], sub, `${path}.${key}`));
    }
    if (schema.additionalProperties && typeof schema.additionalProperties === "object") {
      for (const [key, val] of Object.entries(instance)) {
        if (!(schema.properties && key in schema.properties)) {
          errors.push(...validateAgainstSchema(val, schema.additionalProperties, `${path}.${key}`));
        }
      }
    }
  }
  if (Array.isArray(instance) && schema.items) {
    instance.forEach((item, i) => errors.push(...validateAgainstSchema(item, schema.items, `${path}[${i}]`)));
  }
  return errors;
}

export function loadPublicRegistryTexts(root) {
  const dir = join(root, PUBLIC_REGISTRY_DIR);
  const texts = [];
  if (!existsSync(dir)) return texts;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.isFile() && e.name.endsWith(".json")) {
      texts.push({
        name: `${PUBLIC_REGISTRY_DIR}/${e.name}`,
        text: readFileSync(join(dir, e.name), "utf8"),
      });
    }
  }
  return texts;
}

export function validateFixtures({ root, manifest }) {
  const errors = [];
  const warnings = [];
  const add = (code, message, path) => errors.push({ code, message, ...(path ? { path } : {}) });

  if (!manifest || manifest.artifact !== "membrane.native-contract-fixtures" || manifest.schemaVersion !== 1) {
    add("MANIFEST_SCHEMA_MISMATCH", "native-contract fixture manifest artifact/schemaVersion mismatch");
    return { errors, warnings };
  }

  // Hash integrity over every declared fixture file + corpus README.
  const allFiles = new Set();
  for (const c of manifest.contracts ?? []) {
    if (c.visibility !== "internal") add("VISIBILITY_NOT_INTERNAL", `contract ${c.name} must be classified internal`);
    for (const entry of c.fixtureFiles ?? []) {
      const f = typeof entry === "string" ? entry : entry.path;
      allFiles.add(f);
      if (!existsSync(join(root, f))) {
        add("FIXTURE_MISSING", "declared fixture file absent", f);
        continue;
      }
      if (typeof entry === "object" && entry.sha256) {
        const actual = fixtureSha256(join(root, f));
        if (actual !== entry.sha256) {
          add("FIXTURE_HASH_MISMATCH", `recorded ${entry.sha256.slice(0, 12)}… != actual ${actual.slice(0, 12)}… — in-place edits are forbidden; roll corpus ${manifest.corpusId ? manifest.corpusId.replace(/v\d+$/, "v" + (Number(manifest.corpusId.slice(-1)) + 1)) : "v+1"} with a recorded reason`, f);
        }
      }
    }
  }
  for (const c of manifest.contracts ?? []) {
    const pathsOf = (c.fixtureFiles ?? []).map((e) => (typeof e === "string" ? e : e.path));
    const schemaFile = pathsOf.find((f) => f.endsWith(".schema.json"));
    const exampleFile = pathsOf.find((f) => f.includes("/examples/"));
    if (exampleFile && schemaFile) {
      try {
        const schema = JSON.parse(readFileSync(join(root, schemaFile), "utf8"));
        const example = JSON.parse(readFileSync(join(root, exampleFile), "utf8"));
        const errs = validateAgainstSchema(example, schema);
        for (const e of errs) add("EXAMPLE_SCHEMA_VIOLATION", e, exampleFile);
        if (c.name === "InsightIssueV1") {
          // Honesty-limit verbatim binding between issue and episode contracts.
          const epSchema = JSON.parse(readFileSync(join(root, `${FIXTURES_DIR}/failure-episode-v1.schema.json`), "utf8"));
          if (!epSchema.properties?.honesty_limit?.minLength) {
            add("CONTRACT_BINDING_MISSING", "FailureEpisodeV1 schema lost its honesty_limit binding");
          }
        }
      } catch (e) {
        add("FIXTURE_UNPARSEABLE", e.message, exampleFile ?? schemaFile);
      }
    }
  }

  // Aggregate digest reproduction (over every .json/.md under the corpus dir).
  const dir = join(root, FIXTURES_DIR);
  if (existsSync(dir)) {
    const onDisk = [];
    const walk = (d) => {
      for (const e of readdirSync(d, { withFileTypes: true })) {
        const full = join(d, e.name);
        if (e.isDirectory()) walk(full);
        else {
          const rel = full.slice(root.length + 1).split("\\").join("/");
          if (rel.endsWith(".json") || rel.endsWith(".md")) onDisk.push(rel);
        }
      }
    };
    walk(dir);
    for (const f of onDisk) {
      if (!allFiles.has(f) && !f.endsWith("README.md") && !f.endsWith("manifest.v1.json") && !f.includes("native-contracts-manifest")) {
        warnings.push({ code: "UNDECLARED_FIXTURE_FILE", message: f });
      }
    }
    if (manifest.aggregateSha256 && manifest.aggregateSha256 !== aggregateDigest(root, onDisk)) {
      add("STALE_AGGREGATE_DIGEST", "aggregateSha256 does not match current corpus contents — regenerate or roll a new corpus version");
    }
  }

  // Internal contracts must NOT appear in the public registry.
  for (const { name, text } of loadPublicRegistryTexts(root)) {
    for (const internalName of INTERNAL_CONTRACT_NAMES) {
      if (text.includes(`"${internalName}"`) || text.includes(internalName)) {
        add(
          "INTERNAL_CONTRACT_IN_PUBLIC_REGISTRY",
          `${internalName} must stay an internal domain contract but appears in public registry file`,
          name,
        );
      }
    }
  }

  // The five public V1 shapes remain present and unchanged in name.
  const registryNames = loadPublicRegistryTexts(root).map((t) => t.name).join("\n");
  const toKebab = (s) => s.replace(/V1$/, "").replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase();
  for (const pubShape of PUBLIC_V1_SHAPES) {
    const kebab = toKebab(pubShape);
    if (!registryNames.toLowerCase().includes(kebab) && !registryNames.includes(pubShape)) {
      warnings.push({ code: "PUBLIC_SHAPE_NAME_NOT_FOUND", message: `${pubShape} not found by filename heuristic in ${PUBLIC_REGISTRY_DIR}/` });
    }
  }

  return { errors, warnings };
}

function main(argv) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const jsonOut = argv.includes("--json");
  const update = argv.includes("--update");
  const write = argv.includes("--write");

  if (update) {
    const manifest = buildManifest({ root });
    const out = `${JSON.stringify(manifest, null, 1)}\n`;
    if (write) {
      writeFileSync(join(root, MANIFEST_REL), out);
      process.stdout.write(`wrote ${MANIFEST_REL}\n`);
    } else {
      process.stdout.write(out);
    }
    return 0;
  }

  let manifest;
  try {
    manifest = JSON.parse(readFileSync(join(root, MANIFEST_REL), "utf8"));
  } catch {
    process.stderr.write(`native-contract fixture manifest missing/unreadable: ${MANIFEST_REL}\n`);
    return 2;
  }

  const { errors, warnings } = validateFixtures({ root, manifest });
  const allErrors = [...errors];

  if (jsonOut) {
    process.stdout.write(`${JSON.stringify({ ok: allErrors.length === 0, errors: allErrors, warnings }, null, 2)}\n`);
  } else {
    process.stdout.write(
      `native contract fixture check [corpus ${manifest.corpusId ?? "?"}]\n` +
      `  contracts=${(manifest.contracts ?? []).length} errors=${allErrors.length} warnings=${warnings.length}\n`,
    );
    for (const e of allErrors) process.stdout.write(`  ERROR ${e.code}: ${e.message}${e.path ? ` (${e.path})` : ""}\n`);
    for (const w of warnings) process.stdout.write(`  WARN  ${w.code}: ${w.message}\n`);
  }
  return allErrors.length > 0 ? 1 : 0;
}

function readdirRecursiveJsonMd(dir, root) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, e.name);
    if (e.isDirectory()) out.push(...readdirRecursiveJsonMd(full, root));
    else {
      const rel = full.slice(root.length + 1).split("\\").join("/");
      if (rel.endsWith(".json") || rel.endsWith(".md")) out.push(rel);
    }
  }
  return out.sort();
}

if (process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href) {
  process.exitCode = main(process.argv.slice(2));
}
