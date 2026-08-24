#!/usr/bin/env node
// Runtime-language manifest checker (migration spec N0/N1, sections 16.2/19).
// Pure functions are exported for adversarial tests; CLI modes:
//   node scripts/ci/check-runtime-language-manifest.mjs            -> --check
//   ... --update [--write]   regenerate manifest JSON (stdout unless --write)
//   ... --json               machine-readable report
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const POLICY_REL = "migration/native-rust/runtime-policy.json";
const MANIFEST_REL = "migration/native-rust/runtime-language-manifest.json";
const VALID_DISPOSITIONS = [
  "native-port", "declarative-data", "external-typed-service",
  "external-target-tool", "dev-only", "migration-oracle", "delete",
];
const VALID_RUNTIMES = ["rust", "python", "node", "shell", "declarative", "external"];

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

export function fileSha256(root, rel) {
  return sha256(readFileSync(join(root, rel)));
}

export function aggregateDigest(root, files) {
  const lines = [...files].sort().map((f) => `${f}:${fileSha256(root, f)}\n`);
  return sha256(lines.join(""));
}

function hasShebang(root, rel) {
  try {
    const fd = readFileSync(join(root, rel));
    return fd.length >= 2 && fd[0] === 0x23 && fd[1] === 0x21;
  } catch {
    return false;
  }
}

export function isExecutableCandidate(rel, policy, root) {
  const exts = policy.scan.executableExtensions ?? [];
  if (exts.some((e) => rel.endsWith(e))) return true;
  if (policy.scan.shebangFallback && root) return hasShebang(root, rel);
  return false;
}

export function discoverExecutables(trackedFiles, policy, root) {
  return trackedFiles.filter((f) => isExecutableCandidate(f, policy, root)).sort();
}

export function firstMatchingRule(policy, relPath) {
  for (const rule of policy.classificationRules ?? []) {
    if (new RegExp(rule.pattern).test(relPath)) return rule;
  }
  return null;
}

function groupKeyFor(rule, relPath) {
  if (rule.groupBy === "all") return rule.id;
  if (rule.groupBy === "dir:2") return relPath.split("/").slice(0, 2).join("/");
  if (rule.groupBy === "dir:3") return relPath.split("/").slice(0, 3).join("/");
  return relPath;
}

export function buildRows(root, policy, trackedFiles) {
  const claimed = new Set();
  const buckets = new Map();
  for (const f of discoverExecutables(trackedFiles, policy, root)) {
    const rule = firstMatchingRule(policy, f);
    if (!rule) continue; // left unclassified on purpose -> checker reports it
    if (claimed.has(f)) continue;
    claimed.add(f);
    const key = `${rule.id}::${rule.groupBy === "single" ? f : groupKeyFor(rule, f)}`;
    if (!buckets.has(key)) {
      buckets.set(key, {
        id: key.replace(/[^A-Za-z0-9._\/-]+/g, "-"),
        rule: rule.id,
        runtime: rule.runtime,
        kind: rule.kind,
        product_surface: rule.product_surface ?? "",
        production_reachable: !!rule.production_reachable,
        packaged: rule.packaged ?? false,
        target_disposition: rule.target_disposition,
        owner: rule.owner ?? null,
        exception: rule.exception ?? null,
        parity_fixture: rule.parity_fixture ?? null,
        deletion_or_exclusion_proof: rule.deletion_or_exclusion_proof ?? null,
        seal_blocker_note: rule.seal_blocker_note ?? null,
        notes: rule.notes ?? "",
        files: [],
      });
    }
    buckets.get(key).files.push(f);
  }
  const rows = [...buckets.values()].sort((a, b) => a.id.localeCompare(b.id));
  for (const row of rows) {
    row.files.sort();
    row.content_digest = aggregateDigest(root, row.files);
    if (row.exception) {
      const exc = (policy.exceptions ?? []).find((e) => e.id === row.exception);
      row.expiry = exc ? exc.expires : null;
      row.owner = row.owner ?? exc?.owner ?? null;
    }
  }
  return { rows, claimed };
}

export function buildManifest({ root, policy, trackedFiles, now = new Date(), policyDigest = null }) {
  const { rows } = buildRows(root, policy, trackedFiles);
  const byRuntime = {};
  const byDisposition = {};
  let productionInterpreterRows = 0;
  let fileCount = 0;
  for (const row of rows) {
    byRuntime[row.runtime] = (byRuntime[row.runtime] ?? 0) + 1;
    byDisposition[row.target_disposition] = (byDisposition[row.target_disposition] ?? 0) + 1;
    fileCount += row.files.length;
    if (row.production_reachable && policy.interpreterRuntimes.includes(row.runtime)) {
      productionInterpreterRows += 1;
    }
  }
  return {
    schemaVersion: 1,
    artifact: "membrane.runtime-language-manifest",
    generatedAt: now.toISOString(),
    enforcementMode: policy.enforcementMode,
    policyDigest: policyDigest ?? safePolicyDigest(root),
    totals: {
      files: fileCount,
      rows: rows.length,
      byRuntime,
      byDisposition,
      productionInterpreterRows,
    },
    rows,
  };
}

function safePolicyDigest(root) {
  try {
    return sha256(readFileSync(join(root, POLICY_REL)));
  } catch {
    return null;
  }
}

export function checkGeneratedTruth(texts, flag) {
  const violations = [];
  if (!flag?.complete) return violations;
  for (const retired of flag.retiredAuthorityPaths ?? []) {
    for (const { name, text } of texts) {
      if (text.includes(retired)) {
        violations.push({ flagId: flag.id, truth: name, retiredAuthority: retired });
      }
    }
  }
  return violations;
}

export function findDeletedSelectors(discovered, deletedSelectors) {
  const hits = [];
  for (const selector of deletedSelectors ?? []) {
    for (const f of discovered) {
      if (f === selector || f.startsWith(selector)) hits.push({ selector, path: f });
    }
  }
  return hits;
}

function exceptionPrefixMatch(exc, files) {
  return files.some((f) =>
    (exc.paths ?? []).some((p) => f === p || f.startsWith(p)),
  );
}

export function validateManifest({
  policy, policyDigestActual, manifest, discovered, truthTexts, today, root,
}) {
  const errors = [];
  const warnings = [];
  const add = (code, message, path) => errors.push({ code, message, ...(path ? { path } : {}) });

  if (!manifest || manifest.schemaVersion !== 1 || manifest.artifact !== "membrane.runtime-language-manifest") {
    add("MANIFEST_SCHEMA_MISMATCH", "manifest artifact/schemaVersion does not match policy expectation");
    return { errors, warnings };
  }
  if (manifest.enforcementMode !== policy.enforcementMode) {
    add("MANIFEST_SCHEMA_MISMATCH", `manifest enforcementMode ${manifest.enforcementMode} != policy ${policy.enforcementMode}`);
  }
  if (policyDigestActual && manifest.policyDigest !== policyDigestActual) {
    add("STALE_POLICY_DIGEST", "runtime-policy.json changed since manifest generation; rerun --update");
  }

  const covered = new Map();
  for (const row of manifest.rows ?? []) {
    if (!VALID_DISPOSITIONS.includes(row.target_disposition)) {
      add("INVALID_DISPOSITION", `disposition ${row.target_disposition}`, row.id);
    }
    if (!VALID_RUNTIMES.includes(row.runtime)) {
      add("INVALID_RUNTIME", `runtime ${row.runtime}`, row.id);
    }
    for (const f of row.files ?? []) {
      if (covered.has(f)) add("DUPLICATE_COVERAGE", `also covered by ${covered.get(f)}`, f);
      else covered.set(f, row.id);
    }
    // stale digest
    try {
      const current = aggregateDigest(root, row.files);
      if (current !== row.content_digest) {
        add("STALE_DIGEST", `content changed since manifest generation`, row.id);
      }
    } catch (e) {
      add("STALE_DIGEST", `file unreadable: ${e.message}`, row.id);
    }
    // production language rules
    const interpreterProduction =
      row.production_reachable && (policy.interpreterRuntimes ?? []).includes(row.runtime);
    if (interpreterProduction) {
      if (policy.enforcementMode === "sealed") {
        add("SEALED_MODE_INTERPRETER_PRODUCTION", `sealed mode forbids ${row.runtime} production row`, row.id);
      } else {
        if (!row.exception) {
          add("DISALLOWED_PRODUCTION_LANGUAGE", `${row.runtime} production row without bounded exception`, row.id);
        } else {
          const exc = (policy.exceptions ?? []).find((e) => e.id === row.exception);
          if (!exc) {
            add("MISSING_EXCEPTION_REFERENCE", `exception ${row.exception} not in policy`, row.id);
          } else {
            if (!exc.owner || !exc.expires) {
              add("EXCEPTION_MISSING_OWNER_OR_EXPIRY", `exception ${exc.id} lacks owner/expiry`, row.id);
            }
            if (String(today) >= String(exc.expires)) {
              add("EXCEPTION_EXPIRED", `exception ${exc.id} expired ${exc.expires}`, row.id);
            }
            if (!exceptionPrefixMatch(exc, row.files)) {
              add("EXCEPTION_SCOPE_MISMATCH", `row files outside exception ${exc.id} paths`, row.id);
            }
          }
        }
      }
    }
  }

  for (const f of discovered) {
    if (!covered.has(f)) add("UNCLASSIFIED_EXECUTABLE", "no manifest row covers this executable", f);
  }

  for (const hit of findDeletedSelectors(discovered, policy.deletedSelectors)) {
    add("DELETED_SELECTOR_PRESENT", `deleted selector reappeared at ${hit.path}`, hit.selector);
  }

  for (const flag of policy.cutoverFlags ?? []) {
    for (const v of checkGeneratedTruth(truthTexts ?? [], flag)) {
      add(
        "GENERATED_TRUTH_NAMES_RETIRED_AUTHORITY",
        `${v.truth} still names retired authority ${v.retiredAuthority} after ${v.flagId} completed`,
        v.truth,
      );
    }
  }

  return { errors, warnings };
}

export function evaluateSealReadiness({ policy, manifest, existsFile }) {
  const blockers = [];
  if (policy.enforcementMode !== "sealed") {
    blockers.push(`policy enforcementMode is '${policy.enforcementMode}', not 'sealed'`);
  }
  for (const flag of policy.cutoverFlags ?? []) {
    if (!flag.complete) blockers.push(`cutover flag incomplete: ${flag.id}`);
  }
  for (const row of manifest.rows ?? []) {
    const interpreterProduction =
      row.production_reachable && (policy.interpreterRuntimes ?? []).includes(row.runtime);
    if (interpreterProduction) {
      blockers.push(`production ${row.runtime} row remains: ${row.id}`);
    }
    if (
      ["dev-only", "migration-oracle", "delete"].includes(row.target_disposition) &&
      !row.deletion_or_exclusion_proof
    ) {
      blockers.push(`missing deletion/exclusion proof: ${row.id}`);
    }
    if (interpreterProduction && row.packaged === "unknown") {
      blockers.push(`packaged state unresolved: ${row.id}`);
    }
    if (row.parity_fixture && String(row.parity_fixture).startsWith("planned:")) {
      const p = String(row.parity_fixture).slice("planned:".length);
      if (!existsFile(p)) blockers.push(`parity fixture missing: ${p} (${row.id})`);
    }
    if (row.seal_blocker_note) blockers.push(`${row.id}: ${row.seal_blocker_note}`);
  }
  return blockers;
}

function loadTrackedFiles(root) {
  const res = spawnSync("git", ["ls-files"], { cwd: root, encoding: "utf8" });
  if (res.status !== 0) throw new Error(`git ls-files failed: ${res.stderr}`);
  return res.stdout.split("\n").filter(Boolean);
}

function loadTruthTexts(root, policy) {
  const names = new Set();
  for (const flag of policy.cutoverFlags ?? []) {
    for (const g of flag.generatedTruthGlobs ?? []) names.add(g);
  }
  const texts = [];
  for (const name of [...names].sort()) {
    if (existsSync(join(root, name))) {
      texts.push({ name, text: readFileSync(join(root, name), "utf8") });
    }
  }
  return texts;
}

function computeDigestMap(manifest) {
  // digests are recomputed inside validateManifest via aggregateDigest; kept for interface parity
  return null;
}

function main(argv) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const jsonOut = argv.includes("--json");
  const update = argv.includes("--update");
  const write = argv.includes("--write");
  const policy = JSON.parse(readFileSync(join(root, POLICY_REL), "utf8"));
  const policyDigest = sha256(readFileSync(join(root, POLICY_REL)));
  const today = new Date().toISOString().slice(0, 10);

  if (update) {
    const manifest = buildManifest({
      root, policy, trackedFiles: loadTrackedFiles(root), policyDigest,
    });
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
    process.stderr.write(`runtime-language manifest missing/unreadable: ${MANIFEST_REL}\n`);
    return 2;
  }
  const discovered = discoverExecutables(loadTrackedFiles(root), policy, root);
  const truthTexts = loadTruthTexts(root, policy);
  const { errors, warnings } = validateManifest({
    policy, policyDigestActual: policyDigest, manifest,
    discovered, truthTexts, today, root,
  });
  const blockers = evaluateSealReadiness({ policy, manifest, existsFile: (p) => existsSync(join(root, p)) });

  if (jsonOut) {
    process.stdout.write(`${JSON.stringify({ ok: errors.length === 0, errors, warnings, sealBlockers: blockers }, null, 2)}\n`);
  } else {
    const t = manifest.totals ?? {};
    process.stdout.write(
      `runtime-language manifest check [mode=${policy.enforcementMode}]\n` +
      `  rows=${t.rows ?? "?"} files=${t.files ?? "?"} prodInterpreterRows=${t.productionInterpreterRows ?? "?"}\n` +
      `  errors=${errors.length} warnings=${warnings.length} sealBlockers=${blockers.length}\n`,
    );
    for (const e of errors) process.stdout.write(`  ERROR ${e.code}: ${e.message}${e.path ? ` (${e.path})` : ""}\n`);
    for (const w of warnings) process.stdout.write(`  WARN  ${w.code ?? ""}: ${w.message}\n`);
    if (blockers.length) {
      process.stdout.write(`  seal NOT complete; blockers:\n`);
      for (const b of blockers) process.stdout.write(`    - ${b}\n`);
    }
    process.stdout.write(`  native-only seal: NOT issued (checker never seals; see spec section 16.5)\n`);
  }
  return errors.length > 0 ? 1 : 0;
}

if (process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href) {
  process.exitCode = main(process.argv.slice(2));
}
