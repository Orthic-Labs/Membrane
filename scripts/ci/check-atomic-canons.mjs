#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const atomDir = path.join(root, "docs", "canon");
const pendingPath = path.join(root, "docs", "pending", "README.md");
const atomReadmePath = path.join(atomDir, "README.md");
const preservationPath = path.join(root, "docs", "provenance", "migrations", "2026-08-30-atomic-canons", "preservation-map.md");
const reconciliationPath = path.join(root, "docs", "provenance", "migrations", "2026-08-30-atomic-canons", "source-consumer-reconciliation.md");
const frozenRevision = "d84322c3df182ff1d6ef7ca96fe94aea22273894";
const legacyAtomRevision = "c6cfbca96e5be1d0f8de8cb9614d6158f57cc948";

const canons = Object.freeze([
  { owner: "Membrane", file: "membrane.md", prefix: "MEM", boundary: "RELEASED" },
  { owner: "Pull", file: "pull.md", prefix: "PUL", boundary: "RELEASED" },
  { owner: "Push", file: "push.md", prefix: "PSH", boundary: "RELEASED" },
  { owner: "Cortex", file: "cortex.md", prefix: "CTX", boundary: "RELEASED" },
  { owner: "Blueprint", file: "blueprint.md", prefix: "BPT", boundary: "RELEASED" },
  { owner: "Ledger", file: "ledger.md", prefix: "LDG", boundary: "RELEASED" },
  { owner: "Adapt", file: "adapt.md", prefix: "ADP", boundary: "RELEASED" },
]);

const headers = Object.freeze({
  group: ["ID", "Parent", "Owner", "Scope", "Derived rollup"],
  capability: ["ID", "Parent", "Owner", "Scope", "Observable behavior", "Implementation", "Verification", "Qualification", "Delivery", "Action", "Evidence", "Competitive", "Comparison"],
  implementation: ["ID", "Capability targets", "Mechanism", "Source/donor", "Reuse mode", "State", "Production consumer"],
  qualification: ["ID", "Capability targets", "Acceptance boundary", "State", "Evidence", "Material revision"],
  decision: ["ID", "Kind", "Capability targets", "Decision", "Authority/evidence", "State"],
  preservation: ["Legacy key", "Legacy location", "Old ID", "New kind", "Target/parent", "Disposition", "Ambiguity"],
  split: ["Legacy capability", "Retained ID/behavior", "Introduced ID", "Introduced behavior", "Invariant"],
  introduction: ["Introduced ID", "Origin", "Observable behavior", "Authority/evidence"],
  reconciliation: ["Capability", "State", "Exact source", "Exact consumer", "Residual"],
  focusedVerification: ["Capability targets", "Focused command", "Direct test evidence", "Result", "Run identity/time"],
  comparison: ["Atom", "Scope", "Competitive disposition", "Best mechanism", "Current evidence", "Donor evidence", "Gap / action"],
});

const enums = Object.freeze({
  Scope: new Set(["COMMITTED", "EXPLORATORY", "BACKLOG", "EXCLUDED"]),
  Implementation: new Set(["MISSING", "PARTIAL", "DELIVERED", "UNKNOWN"]),
  Verification: new Set(["PENDING", "FOCUSED_PASS", "FAIL", "STALE", "UNKNOWN"]),
  Qualification: new Set(["NOT_REQUIRED", "PENDING", "PASS", "FAIL", "STALE", "UNKNOWN"]),
  Delivery: new Set(["LOCAL", "COMMITTED", "PUSHED", "RELEASED", "UNKNOWN"]),
  Competitive: new Set(["CURRENT_BEST", "DONOR_BETTER", "CURRENT_INCOMPLETE", "UNRESOLVED", "NOT_COMMITTED"]),
  decisionKind: new Set(["REFERENCE", "EXCLUSION", "BACKLOG"]),
});
const deliveryRank = Object.freeze({ UNKNOWN: -1, LOCAL: 0, COMMITTED: 1, PUSHED: 2, RELEASED: 3 });

function cells(line) { return line.trim().slice(1, -1).split("|").map((cell) => cell.trim()); }
function isSeparator(row) { return row.every((cell) => /^:?-{3,}:?$/.test(cell)); }
function tableAfter(markdown, heading) {
  const normalized = markdown.replace(/\r\n/g, "\n");
  const start = normalized.indexOf(`${heading}\n`);
  if (start < 0) throw new Error(`missing heading ${heading}`);
  const rows = [];
  let started = false;
  for (const line of normalized.slice(start + heading.length).split("\n")) {
    if (!started && line.trim().startsWith("|")) started = true;
    if (started && !line.trim().startsWith("|")) break;
    if (started) rows.push(cells(line));
  }
  if (rows.length < 2) throw new Error(`missing table after ${heading}`);
  return rows;
}
function records(markdown, heading, expected) {
  const table = tableAfter(markdown, heading);
  if (table[0].join("|") !== expected.join("|")) throw new Error(`${heading} schema mismatch: ${table[0].join(" | ")}`);
  return table.slice(1).filter((row) => !isSeparator(row)).map((row) => {
    if (row.length !== expected.length) throw new Error(`${heading}: expected ${expected.length} fields, got ${row.length}`);
    return Object.fromEntries(expected.map((name, index) => [name, row[index]]));
  });
}
function targets(value) { return !value || value === "—" ? [] : value.split(",").map((target) => target.trim()).filter(Boolean); }
function proofEvidence(value) {
  const match = /^Acceptance: ([A-Z][A-Z0-9-]+); Revision: ([0-9a-f]{40}); Receipt: ([^;]+@[0-9a-f]{8,64}); Freshness: (\d{4}-\d{2}-\d{2})$/.exec(value);
  if (!match) return null;
  const freshness = Date.parse(`${match[4]}T00:00:00Z`);
  if (!Number.isFinite(freshness) || freshness > Date.now()) return null;
  return { acceptance: match[1], revision: match[2], receipt: match[3], freshness: match[4] };
}
function recordsByHeader(markdown, expected) {
  const lines = markdown.replace(/\r\n/g, "\n").split("\n");
  const start = lines.findIndex((line) => line.trim().startsWith("|") && cells(line).join("|") === expected.join("|"));
  if (start < 0) throw new Error(`missing table with schema ${expected.join(" | ")}`);
  const table = [];
  for (const line of lines.slice(start)) {
    if (!line.trim().startsWith("|")) break;
    table.push(cells(line));
  }
  return table.slice(1).filter((row) => !isSeparator(row)).map((row) => {
    if (row.length !== expected.length) throw new Error(`table: expected ${expected.length} fields, got ${row.length}`);
    return Object.fromEntries(expected.map((name, index) => [name, row[index]]));
  });
}
function comparisonEvidence(value) {
  const match = /^Receipt: ([^;]+)@([0-9a-f]{40}); Atom: ([A-Z]{3}-\d{3}); Compared: ([0-9a-f]{40})$/.exec(value);
  return match ? { relative: match[1], hash: match[2], atom: match[3], compared: match[4] } : null;
}

const liveLocatorLineCounts = new Map();
function validateLiveLocators(value, label) {
  const pattern = /((?:engine|blueprint|scripts|docs|migration|schemas|mcp|clients)\/[A-Za-z0-9_./@+-]+):(\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)/g;
  for (const match of value.matchAll(pattern)) {
    const relative = match[1], absolute = path.join(root, relative);
    if (!existsSync(absolute)) throw new Error(`${label}: locator path does not exist: ${relative}`);
    if (!liveLocatorLineCounts.has(relative)) liveLocatorLineCounts.set(relative, readFileSync(absolute, "utf8").replace(/\r\n/g, "\n").split("\n").length);
    const maximum = Math.max(...match[2].split(",").map((range) => Number.parseInt(range.split("-").at(-1), 10)));
    if (maximum > liveLocatorLineCounts.get(relative)) throw new Error(`${label}: locator line ${maximum} exceeds ${relative}`);
  }
}

function parseCanon(config) {
  const markdown = readFileSync(path.join(atomDir, config.file), "utf8");
  const boundary = /Required delivery boundary: `(LOCAL|COMMITTED|PUSHED|RELEASED)`\./.exec(markdown)?.[1];
  if (boundary !== config.boundary) throw new Error(`${config.file}: required delivery boundary must be ${config.boundary}`);
  const groups = records(markdown, "## Group register", headers.group);
  const capabilities = records(markdown, "## Capability ledger", headers.capability);
  const implementations = records(markdown, "## Implementation register", headers.implementation);
  const qualifications = records(markdown, "## Qualification ledger", headers.qualification);
  const decisions = records(markdown, "## Decision register", headers.decision);
  const groupIds = new Set(groups.map((row) => row.ID));
  const capabilityIds = new Set(capabilities.map((row) => row.ID));
  for (const row of groups) {
    if (!new RegExp(`^${config.prefix}-G\\d{2}$`).test(row.ID)) throw new Error(`${config.file}: invalid group ID ${row.ID}`);
    if (row.Owner !== config.owner) throw new Error(`${config.file}:${row.ID}: owner ${row.Owner} != ${config.owner}`);
    if (!enums.Scope.has(row.Scope)) throw new Error(`${config.file}:${row.ID}: invalid Scope ${row.Scope}`);
    if (row.Parent !== "—" && !groupIds.has(row.Parent)) throw new Error(`${config.file}:${row.ID}: unknown parent ${row.Parent}`);
  }
  const groupById = new Map(groups.map((row) => [row.ID, row]));
  for (const row of groups) {
    const seen = new Set([row.ID]);
    let parent = row.Parent;
    while (parent !== "—") {
      if (seen.has(parent)) throw new Error(`${config.file}:${row.ID}: cyclic group parentage`);
      seen.add(parent);
      parent = groupById.get(parent).Parent;
    }
  }
  for (const row of capabilities) {
    if (!new RegExp(`^${config.prefix}-\\d{3}$`).test(row.ID)) throw new Error(`${config.file}: invalid capability ID ${row.ID}`);
    if (row.Owner !== config.owner) throw new Error(`${config.file}:${row.ID}: owner ${row.Owner} != ${config.owner}`);
    if (!groupIds.has(row.Parent)) throw new Error(`${config.file}:${row.ID}: unknown parent ${row.Parent}`);
    for (const field of ["Scope", "Implementation", "Verification", "Qualification", "Delivery", "Competitive"]) {
      if (!enums[field].has(row[field])) throw new Error(`${config.file}:${row.ID}: invalid ${field} ${row[field]}`);
    }
    if (!row["Observable behavior"] || !row.Action || !row.Evidence || !row.Comparison) throw new Error(`${config.file}:${row.ID}: incomplete capability row`);
    if ((row.Verification === "FOCUSED_PASS" || row.Qualification === "PASS") && !proofEvidence(row.Evidence)) throw new Error(`${config.file}:${row.ID}: PASS state lacks exact evidence`);
    if (row.Qualification === "NOT_REQUIRED") {
      const disposition = qualifications.find((qualification) => targets(qualification["Capability targets"]).includes(row.ID) && qualification.State === "NOT_REQUIRED");
      if (!disposition || !proofEvidence(disposition.Evidence)) throw new Error(`${config.file}:${row.ID}: NOT_REQUIRED lacks revision-bound disposition`);
    }
    if (!comparisonEvidence(row.Comparison)) throw new Error(`${config.file}:${row.ID}: invalid comparison evidence`);
    if (row.Scope === "COMMITTED" && row.Competitive === "NOT_COMMITTED") throw new Error(`${config.file}:${row.ID}: committed capability cannot be NOT_COMMITTED`);
    if (row.Scope !== "COMMITTED" && row.Competitive !== "NOT_COMMITTED") throw new Error(`${config.file}:${row.ID}: non-committed capability must be NOT_COMMITTED`);
    if (row.Competitive === "CURRENT_BEST" && (row.Implementation !== "DELIVERED" || row.Verification !== "FOCUSED_PASS")) throw new Error(`${config.file}:${row.ID}: CURRENT_BEST requires delivered implementation and focused pass`);
    if (row.Competitive === "CURRENT_INCOMPLETE" && !["PARTIAL", "MISSING"].includes(row.Implementation)) throw new Error(`${config.file}:${row.ID}: CURRENT_INCOMPLETE requires partial or missing implementation`);
  }
  const validateTargets = (rows, kind, pattern) => {
    for (const row of rows) {
      if (!pattern.test(row.ID)) throw new Error(`${config.file}: invalid ${kind} ID ${row.ID}`);
      if (!targets(row["Capability targets"]).length) throw new Error(`${config.file}:${row.ID}: ${kind} lacks capability target`);
      for (const target of targets(row["Capability targets"])) if (!capabilityIds.has(target)) throw new Error(`${config.file}:${row.ID}: unresolved target ${target}`);
    }
  };
  validateTargets(implementations, "implementation", new RegExp(`^${config.prefix}-I\\d{3}$`));
  validateTargets(qualifications, "qualification", new RegExp(`^${config.prefix}-Q\\d{3}$`));
  for (const row of implementations) {
    if (!enums.Implementation.has(row.State)) throw new Error(`${config.file}:${row.ID}: invalid implementation state ${row.State}`);
    if (!row.Mechanism || !row["Source/donor"] || !row["Reuse mode"] || !row["Production consumer"]) throw new Error(`${config.file}:${row.ID}: incomplete implementation row`);
    validateLiveLocators(row.Mechanism, `${config.file}:${row.ID}`);
    validateLiveLocators(row["Production consumer"], `${config.file}:${row.ID}`);
  }
  for (const row of qualifications) {
    if (!enums.Qualification.has(row.State)) throw new Error(`${config.file}:${row.ID}: invalid qualification state ${row.State}`);
    if (row.State === "PASS") {
      const proof = proofEvidence(row.Evidence);
      if (!proof) throw new Error(`${config.file}:${row.ID}: PASS lacks exact evidence`);
      if (proof.revision !== row["Material revision"]) throw new Error(`${config.file}:${row.ID}: PASS predates material revision`);
    }
  }
  for (const row of decisions) {
    if (!new RegExp(`^${config.prefix}-D\\d{3}$`).test(row.ID)) throw new Error(`${config.file}: invalid decision ID ${row.ID}`);
    if (!enums.decisionKind.has(row.Kind)) throw new Error(`${config.file}:${row.ID}: invalid decision kind ${row.Kind}`);
  }
  return { ...config, groups, capabilities, implementations, qualifications, decisions };
}

function closed(row, boundary) {
  return row.Scope === "COMMITTED" && row.Implementation === "DELIVERED" && row.Verification === "FOCUSED_PASS"
    && ["PASS", "NOT_REQUIRED"].includes(row.Qualification) && deliveryRank[row.Delivery] >= deliveryRank[boundary]
    && Boolean(proofEvidence(row.Evidence));
}
function competitivelyClosed(row) { return row.Scope === "COMMITTED" && row.Competitive === "CURRENT_BEST"; }
function normalizedTokens(value) {
  const stop = new Set(["a", "an", "and", "the", "to", "of", "for", "with", "when", "only", "one", "or", "from", "into", "without"]);
  return new Set(value.toLowerCase().replace(/[^a-z0-9]+/g, " ").split(" ").filter((token) => token.length > 2 && !stop.has(token)));
}
function similarity(a, b) {
  const left = normalizedTokens(a), right = normalizedTokens(b);
  const intersection = [...left].filter((token) => right.has(token)).length;
  const union = new Set([...left, ...right]).size;
  return union ? intersection / union : 0;
}
function validateIdentity(parsed) {
  const everyId = new Set();
  const capabilityIds = new Set(parsed.flatMap((canon) => canon.capabilities).map((row) => row.ID));
  for (const canon of parsed) {
    for (const rows of [canon.groups, canon.capabilities, canon.implementations, canon.qualifications, canon.decisions]) for (const row of rows) {
      if (everyId.has(row.ID)) throw new Error(`duplicate canon ID ${row.ID}`);
      everyId.add(row.ID);
    }
    const implementationTargets = new Map(), qualificationTargets = new Map();
    for (const row of canon.implementations) {
      if (row["Reuse mode"] === "RECLASSIFIED_IMPLEMENTATION") continue;
      for (const target of targets(row["Capability targets"])) implementationTargets.set(target, (implementationTargets.get(target) ?? 0) + 1);
    }
    for (const row of canon.qualifications) for (const target of targets(row["Capability targets"])) qualificationTargets.set(target, (qualificationTargets.get(target) ?? 0) + 1);
    for (const row of canon.capabilities) {
      if (implementationTargets.get(row.ID) !== 1) throw new Error(`${canon.file}:${row.ID}: expected exactly one implementation row`);
      if (qualificationTargets.get(row.ID) !== 1) throw new Error(`${canon.file}:${row.ID}: expected exactly one qualification row`);
      const implementation = canon.implementations.find((candidate) => candidate["Reuse mode"] !== "RECLASSIFIED_IMPLEMENTATION" && targets(candidate["Capability targets"]).includes(row.ID));
      const qualification = canon.qualifications.find((candidate) => targets(candidate["Capability targets"]).includes(row.ID));
      if (implementation.State !== row.Implementation) throw new Error(`${canon.file}:${row.ID}: capability/register implementation state differs`);
      if (qualification.State !== row.Qualification) throw new Error(`${canon.file}:${row.ID}: capability/register qualification state differs`);
    }
    const committedCount = canon.capabilities.filter((row) => row.Scope === "COMMITTED").length;
    const exploratoryCount = canon.capabilities.filter((row) => row.Scope === "EXPLORATORY").length;
    if (!canon.groups[0]["Derived rollup"].includes(`${committedCount} committed capabilities`)) throw new Error(`${canon.file}: group rollup has stale committed count`);
    if (exploratoryCount && !canon.groups[0]["Derived rollup"].includes(`${exploratoryCount} exploratory capability`)) throw new Error(`${canon.file}: group rollup has stale exploratory count`);
    for (const row of canon.decisions) for (const target of targets(row["Capability targets"])) {
      if (!capabilityIds.has(target)) throw new Error(`${canon.file}:${row.ID}: unresolved target ${target}`);
    }
  }
  return { everyId, capabilityIds };
}

let focusedAssertionCorpus;
function testSourceFiles(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) return testSourceFiles(absolute);
    return entry.isFile() && /\.(?:mjs|rs)$/.test(entry.name) ? [absolute] : [];
  });
}
function focusedProofLooksExact(command, evidence) {
  if (!/^(?:`)?(?:rightkit cargo test|node --test)\b/.test(command) || /\b(?:TBD|TODO|placeholder|unknown)\b/i.test(evidence)) return false;
  const candidates = [...evidence.matchAll(/`([^`]+)`/g)]
    .map((match) => match[1].split("::").at(-1).trim())
    .filter((candidate) => candidate.length >= 8 && !/^(?:TBD|TODO|placeholder|unknown)$/i.test(candidate));
  if (!candidates.length) return false;
  if (focusedAssertionCorpus === undefined) {
    const testSources = [path.join(root, "engine", "crates"), path.join(root, "blueprint", "tests")]
      .flatMap(testSourceFiles)
      .sort();
    focusedAssertionCorpus = testSources.map((file) => readFileSync(file, "utf8")).join("\n");
  }
  return candidates.some((candidate) => focusedAssertionCorpus.includes(candidate));
}
function validateReceiptReferences(parsed) {
  const receipts = new Map();
  for (const canon of parsed) for (const row of canon.capabilities) {
    const proof = proofEvidence(row.Evidence);
    if (!proof) continue;
    if (proof.acceptance !== row.ID) throw new Error(`${canon.file}:${row.ID}: acceptance ID mismatch`);
    const separator = proof.receipt.lastIndexOf("@");
    const relative = proof.receipt.slice(0, separator), expectedHash = proof.receipt.slice(separator + 1);
    const absolute = path.join(root, relative);
    if (!existsSync(absolute)) throw new Error(`${canon.file}:${row.ID}: missing evidence receipt ${relative}`);
    try { execFileSync("git", ["ls-files", "--error-unmatch", "--", relative], { cwd: root, stdio: "ignore" }); }
    catch { throw new Error(`${canon.file}:${row.ID}: evidence receipt is not tracked/staged ${relative}`); }
    try { execFileSync("git", ["cat-file", "-e", `${proof.revision}^{commit}`], { cwd: root, stdio: "ignore" }); }
    catch { throw new Error(`${canon.file}:${row.ID}: evidence revision is not a commit ${proof.revision}`); }
    const actualHash = execFileSync("git", ["hash-object", absolute], { cwd: root, encoding: "utf8" }).trim();
    if (actualHash !== expectedHash) throw new Error(`${canon.file}:${row.ID}: evidence receipt hash drift`);
    if (!receipts.has(relative)) {
      const markdown = readFileSync(absolute, "utf8");
      const rows = markdown.replace(/\r\n/g, "\n").split("\n")
        .filter((line) => /^\| [A-Z]{3}-\d{3} \| (?:DELIVERED|PARTIAL|MISSING) \|/.test(line))
        .map((line) => Object.fromEntries(headers.reconciliation.map((name, index) => [name, cells(line)[index]])));
      const byCapability = new Map();
      for (const receiptRow of rows) {
        validateLiveLocators(receiptRow["Exact source"], `${relative}:${receiptRow.Capability}:source`);
        validateLiveLocators(receiptRow["Exact consumer"], `${relative}:${receiptRow.Capability}:consumer`);
        if (byCapability.has(receiptRow.Capability)) throw new Error(`${relative}: duplicate receipt row ${receiptRow.Capability}`);
        byCapability.set(receiptRow.Capability, receiptRow);
      }
      receipts.set(relative, byCapability);
    }
    const receiptRow = receipts.get(relative).get(row.ID);
    if (!receiptRow) throw new Error(`${canon.file}:${row.ID}: evidence receipt lacks acceptance row`);
    if (receiptRow.State !== row.Implementation) throw new Error(`${canon.file}:${row.ID}: receipt implementation state differs`);
    if (row.Implementation === "DELIVERED" && receiptRow.Residual !== "COMPLETE") throw new Error(`${canon.file}:${row.ID}: delivered receipt residual must be COMPLETE`);
  }
  const focused = records(readFileSync(reconciliationPath, "utf8"), "## Focused verification", headers.focusedVerification);
  const focusedById = new Map();
  const capabilityById = new Map(parsed.flatMap((canon) => canon.capabilities).map((row) => [row.ID, row]));
  for (const row of focused) {
    const ids = targets(row["Capability targets"]);
    if (ids.length !== 1) throw new Error("focused verification receipt must have exactly one capability per row");
    const id = ids[0];
    const capability = capabilityById.get(id);
    if (!capability) throw new Error(`focused verification receipt targets unknown capability ${id}`);
    if (capability.Verification !== "FOCUSED_PASS") continue;
    if (focusedById.has(id)) throw new Error(`focused verification receipt duplicates ${id}`);
    const proofText = `${row["Focused command"]} ${row["Direct test evidence"]} ${row["Run identity/time"]}`;
    if (!row["Focused command"] || !row["Direct test evidence"] || !row["Run identity/time"] || !/FOCUSED_PASS/.test(row.Result) || !/0 fail/i.test(`${row.Result} ${row["Run identity/time"]}`)) throw new Error(`focused verification receipt lacks exact successful proof for ${id}`);
    if (/exact [A-Z]{3}-\d{3} (?:focused|behavior) assertion|focused suites|recorded focused run|exact focused receipts/i.test(proofText) || !focusedProofLooksExact(row["Focused command"], row["Direct test evidence"])) throw new Error(`focused verification receipt uses placeholder or nonexistent proof for ${id}`);
    focusedById.set(id, row);
  }
  for (const canon of parsed) for (const row of canon.capabilities) if (row.Verification === "FOCUSED_PASS" && !focusedById.has(row.ID)) throw new Error(`${canon.file}:${row.ID}: FOCUSED_PASS lacks focused verification receipt`);
}
function validateComparisonReferences(parsed) {
  const receipts = new Map();
  for (const canon of parsed) for (const row of canon.capabilities) {
    const proof = comparisonEvidence(row.Comparison);
    if (!proof || proof.atom !== row.ID) throw new Error(`${canon.file}:${row.ID}: comparison atom mismatch`);
    if (!receipts.has(proof.relative)) {
      const absolute = path.join(root, proof.relative);
      if (!existsSync(absolute)) throw new Error(`${canon.file}:${row.ID}: missing comparison receipt ${proof.relative}`);
      try { execFileSync("git", ["ls-files", "--error-unmatch", "--", proof.relative], { cwd: root, stdio: "ignore" }); }
      catch { throw new Error(`${canon.file}:${row.ID}: comparison receipt is not tracked/staged ${proof.relative}`); }
      const actualHash = execFileSync("git", ["hash-object", absolute], { cwd: root, encoding: "utf8" }).trim();
      if (actualHash !== proof.hash) throw new Error(`${canon.file}:${row.ID}: comparison receipt hash drift`);
      try { execFileSync("git", ["cat-file", "-e", `${proof.compared}^{commit}`], { cwd: root, stdio: "ignore" }); }
      catch { throw new Error(`${canon.file}:${row.ID}: compared revision is not a commit`); }
      const markdown = readFileSync(absolute, "utf8"), rows = recordsByHeader(markdown, headers.comparison);
      const byAtom = new Map();
      for (const comparison of rows) {
        if (byAtom.has(comparison.Atom)) throw new Error(`${proof.relative}: duplicate comparison row ${comparison.Atom}`);
        byAtom.set(comparison.Atom, comparison);
      }
      receipts.set(proof.relative, { byAtom, hash: actualHash, compared: proof.compared });
    }
    const receipt = receipts.get(proof.relative);
    if (receipt.hash !== proof.hash || receipt.compared !== proof.compared) throw new Error(`${canon.file}:${row.ID}: comparison receipt metadata differs`);
    const comparison = receipt.byAtom.get(row.ID);
    if (!comparison) throw new Error(`${canon.file}:${row.ID}: comparison receipt lacks atom row`);
    if (comparison.Scope !== row.Scope || comparison["Competitive disposition"] !== row.Competitive) throw new Error(`${canon.file}:${row.ID}: comparison receipt state differs`);
  }
}
function behavior(byId, id) {
  const row = byId.get(id);
  if (!row) throw new Error(`missing ownership atom ${id}`);
  return row["Observable behavior"];
}
function validateSemanticOwnership(parsed) {
  const capabilities = parsed.flatMap((canon) => canon.capabilities), byId = new Map(capabilities.map((row) => [row.ID, row]));
  const requireWords = (id, pattern, message) => { if (!pattern.test(behavior(byId, id))) throw new Error(message); };
  const rejectWords = (id, pattern, message) => { if (pattern.test(behavior(byId, id))) throw new Error(message); };
  requireWords("PUL-015", /admit|invok/i, "PUL-015 must own provider admission/invocation");
  rejectWords("PUL-015", /materializ/i, "PUL-015 overlaps LDG-022 materialization");
  requireWords("LDG-022", /materializ/i, "LDG-022 must own candidate materialization");
  rejectWords("LDG-022", /admit|invok/i, "LDG-022 overlaps PUL-015 admission/invocation");
  requireWords("MEM-042", /Membrane-wide|envelope/i, "MEM-042 must own Membrane-wide receipt envelope");
  requireWords("PUL-031", /Pull|candidate journey|packet/i, "PUL-031 must own Pull packet receipt content");
  requireWords("MEM-024", /classif|transport|resol/i, "MEM-024 must own feedback transport & receipt/verdict resolution");
  requireWords("PUL-032", /emit|observation|outcome/i, "PUL-032 must own Pull outcome emission");
  requireWords("CTX-015", /apply|usefulness|recall/i, "CTX-015 must own Cortex feedback application");
  requireWords("ADP-015", /preference|applicability|outcome/i, "ADP-015 must own Adapt preference outcome");
  requireWords("MEM-043", /create|request absolute deadline/i, "MEM-043 must own request deadline creation/propagation");
  requireWords("PUL-005", /provider work|provider caps|inherited request deadline/i, "PUL-005 must own provider scheduling under inherited deadline");
  requireWords("MEM-052", /generic|daemon jobs|admit/i, "MEM-052 must own generic daemon scheduling/admission");
  rejectWords("MEM-052", /learner|proposal sink/i, "MEM-052 overlaps ADP-035 learner semantics");
  requireWords("ADP-035", /learner|proposal sink/i, "ADP-035 must own learner semantics/proposal sink");
  requireWords("MEM-016", /tray-owned daemon|wire compatibility/i, "MEM-016 must distinguish tray daemon from hub_inactive wire compatibility");
  if (byId.get("PUL-001")?.Implementation !== "PARTIAL") throw new Error("PUL-001 must remain PARTIAL until deterministic requirement detail lands");
  if (byId.get("PUL-015")?.Implementation !== "PARTIAL") throw new Error("PUL-015 must remain PARTIAL while only shadow activation exists");
  if (byId.get("MEM-024")?.Implementation !== "PARTIAL") throw new Error("MEM-024 must remain PARTIAL until receipt/verdict resolution is correct");
  if (byId.has("BPT-045")) throw new Error("BPT-045 must remain preservation-only legacy alias");
  const exploratory = capabilities.filter((row) => row.Scope === "EXPLORATORY").map((row) => row.ID).sort();
  const expectedExploratory = ["ADP-065", "ADP-066", "ADP-067", "ADP-068", "ADP-069", "ADP-070", "ADP-071", "BPT-048", "CTX-033", "CTX-039", "LDG-023", "PUL-034"];
  if (JSON.stringify(exploratory) !== JSON.stringify(expectedExploratory)) throw new Error(`exploratory disposition differs: ${exploratory.join(", ")}`);
  for (let left = 0; left < capabilities.length; left += 1) for (let right = left + 1; right < capabilities.length; right += 1) {
    if (similarity(capabilities[left]["Observable behavior"], capabilities[right]["Observable behavior"]) >= 0.9) throw new Error(`semantic duplicate candidates: ${capabilities[left].ID} & ${capabilities[right].ID}`);
  }
}

function validatePreservation(everyId, capabilityIds) {
  const markdown = readFileSync(preservationPath, "utf8");
  const rows = records(markdown, "# Atomic canon preservation map", headers.preservation);
  const legacyAtoms = rows.filter((row) => row["Legacy key"].startsWith("ATOM-"));
  const specRows = rows.filter((row) => row["Legacy key"].startsWith("SPEC-"));
  const unclassified = rows.filter((row) => row["New kind"] === "UNCLASSIFIED" || row.Ambiguity !== "NONE");
  const splits = records(markdown, "## Atomic split register", headers.split);
  const introductions = records(markdown, "## New capability register", headers.introduction);
  if (legacyAtoms.length !== 249 || specRows.length !== 479 || rows.length !== 728) throw new Error(`preservation union mismatch: atoms=${legacyAtoms.length}, specs=${specRows.length}, total=${rows.length}`);
  if (unclassified.length) throw new Error(`preservation map: ${unclassified.length} rows remain unclassified/ambiguous`);
  const keys = new Set(), oldIds = new Set();
  for (const row of rows) {
    if (keys.has(row["Legacy key"])) throw new Error(`preservation map: duplicate key ${row["Legacy key"]}`);
    if (oldIds.has(row["Old ID"])) throw new Error(`preservation map: duplicate old ID ${row["Old ID"]}`);
    keys.add(row["Legacy key"]); oldIds.add(row["Old ID"]);
    if (!["CAPABILITY", "IMPLEMENTATION", "REFERENCE", "BACKLOG", "EXCLUSION"].includes(row["New kind"])) throw new Error(`preservation map:${row["Legacy key"]}: invalid kind ${row["New kind"]}`);
    for (const target of targets(row["Target/parent"])) if (!everyId.has(target)) throw new Error(`preservation map:${row["Legacy key"]}: unresolved target ${target}`);
  }
  const mapped = new Set(legacyAtoms.flatMap((row) => targets(row["Target/parent"])));
  for (const row of legacyAtoms) {
    const locator = /^(.+):(\d+)@([0-9a-f]{40})$/.exec(row["Legacy location"]);
    if (!locator || locator[3] !== legacyAtomRevision) throw new Error(`preservation map:${row["Legacy key"]}: invalid legacy source locator`);
    const source = execFileSync("git", ["show", `${legacyAtomRevision}:${locator[1]}`], { cwd: root, encoding: "utf8" }).split(/\r?\n/);
    if (!source[Number(locator[2]) - 1]?.startsWith(`| ${row["Old ID"]} |`)) throw new Error(`preservation map:${row["Legacy key"]}: source locator does not identify ${row["Old ID"]}`);
    if (row["Old ID"] === "BPT-022" && row["New kind"] === "IMPLEMENTATION" && row["Target/parent"] === "BPT-030, BPT-031, BPT-032") continue;
    if (["ADP-037", "ADP-039", "BPT-045"].includes(row["Old ID"]) && row["New kind"] === "REFERENCE") continue;
    if (row["New kind"] !== "CAPABILITY" || row["Old ID"] !== row["Target/parent"]) throw new Error(`preservation map reclassifies ${row["Old ID"]} without explicit mapping`);
  }
  const frozenAtomIds = new Set();
  for (const [prefix, count] of [["MEM", 54], ["PUL", 34], ["PSH", 17], ["CTX", 34], ["BPT", 48], ["LDG", 23], ["ADP", 39]]) {
    for (let index = 1; index <= count; index += 1) frozenAtomIds.add(`${prefix}-${String(index).padStart(3, "0")}`);
  }
  if (frozenAtomIds.size !== 249) throw new Error(`frozen atom inventory changed: expected 249, found ${frozenAtomIds.size}`);
  const legacySourceCache = new Map();
  for (const row of legacyAtoms) {
    const match = /^(.*):(\d+)@([0-9a-f]{40})$/.exec(row["Legacy location"]);
    if (!match) throw new Error(`preservation map:${row["Legacy key"]}: malformed legacy atom locator`);
    const [, sourcePath, sourceLineText, revision] = match;
    if (revision !== legacyAtomRevision) throw new Error(`preservation map:${row["Legacy key"]}: wrong legacy atom revision ${revision}`);
    const sourceKey = `${revision}:${sourcePath}`;
    if (!legacySourceCache.has(sourceKey)) {
      try { legacySourceCache.set(sourceKey, execFileSync("git", ["show", sourceKey], { cwd: root, encoding: "utf8" }).replace(/\r\n/g, "\n")); }
      catch { throw new Error(`preservation map:${row["Legacy key"]}: unreachable legacy atom source ${sourceKey}`); }
    }
    const sourceLine = Number.parseInt(sourceLineText, 10);
    const lineCount = legacySourceCache.get(sourceKey).split("\n").length;
    if (sourceLine < 1 || sourceLine > lineCount) throw new Error(`preservation map:${row["Legacy key"]}: legacy atom line ${sourceLine} exceeds ${lineCount}`);
  }
  for (const id of frozenAtomIds) {
    const reclassified = legacyAtoms.some((row) => row["Old ID"] === id && (
      (id === "BPT-022" && row["New kind"] === "IMPLEMENTATION")
      || (["ADP-037", "ADP-039", "BPT-045"].includes(id) && row["New kind"] === "REFERENCE")
    ));
    if ((!mapped.has(id) && !reclassified) || !legacyAtoms.some((row) => row["Old ID"] === id)) throw new Error(`preservation map omits frozen atom ${id}`);
  }
  const expectedSplitIds = new Set([
    "MEM-055", "MEM-056", "MEM-057", "MEM-058", "MEM-059", "MEM-060", "MEM-061", "MEM-062", "MEM-063", "MEM-064", "MEM-065", "MEM-066",
    "PUL-035", "PUL-036", "CTX-035", "CTX-036", "CTX-037", "BPT-049", "BPT-050", "BPT-051", "BPT-052", "BPT-053", "BPT-054", "BPT-055", "BPT-056", "BPT-057", "BPT-060", "LDG-024", "LDG-025", "ADP-040",
  ]);
  if (splits.length !== expectedSplitIds.size) throw new Error(`atomic split register: expected ${expectedSplitIds.size} introduced atoms, found ${splits.length}`);
  const introduced = new Set();
  for (const row of splits) {
    if (!frozenAtomIds.has(row["Legacy capability"])) throw new Error(`atomic split register: unknown legacy capability ${row["Legacy capability"]}`);
    if (!row["Retained ID/behavior"].startsWith(row["Legacy capability"])) throw new Error(`atomic split register:${row["Introduced ID"]}: retained behavior loses old ID`);
    if (!capabilityIds.has(row["Introduced ID"]) || frozenAtomIds.has(row["Introduced ID"])) throw new Error(`atomic split register: invalid introduced atom ${row["Introduced ID"]}`);
    if (introduced.has(row["Introduced ID"])) throw new Error(`atomic split register: duplicate introduced atom ${row["Introduced ID"]}`);
    if (!expectedSplitIds.has(row["Introduced ID"])) throw new Error(`atomic split register: unexpected introduced atom ${row["Introduced ID"]}`);
    introduced.add(row["Introduced ID"]);
  }
  for (const id of expectedSplitIds) if (!introduced.has(id)) throw new Error(`atomic split register: missing introduced atom ${id}`);
  for (const row of introductions) {
    const id = row["Introduced ID"];
    if (!capabilityIds.has(id) || frozenAtomIds.has(id) || introduced.has(id)) throw new Error(`new capability register: invalid introduced atom ${id}`);
    if (introduced.has(id)) throw new Error(`new capability register: duplicate introduced atom ${id}`);
    if (!row.Origin || !row["Observable behavior"] || !row["Authority/evidence"] || /pending|unknown/i.test(row["Authority/evidence"])) throw new Error(`new capability register:${id}: incomplete authority`);
    introduced.add(id);
  }
  for (const id of capabilityIds) if (!frozenAtomIds.has(id) && !introduced.has(id)) throw new Error(`capability provenance omits introduced atom ${id}`);
  const frozenPendingFiles = execFileSync("git", ["ls-tree", "-r", "--name-only", frozenRevision, "--", "docs/pending"], { cwd: root, encoding: "utf8" }).split(/\r?\n/).filter((file) => file.endsWith(".md"));
  const specLocations = new Set(specRows.map((row) => row["Legacy location"].replace(/:\d+@(?:LOCAL|[0-9a-f]{40})$/, "")));
  for (const file of frozenPendingFiles) if (!specLocations.has(file)) throw new Error(`preservation map omits frozen pending document ${file}`);
  for (const file of ["docs/pending/capabilities/blueprint/findings-lane.md", "docs/pending/design/membrane-brand-identity.md", "docs/pending/design/hub/hub-mockup.html"]) if (!specLocations.has(file)) throw new Error(`preservation map omits local supporting specification ${file}`);
  for (const [pattern, label] of [[/Legacy atom rows: \*\*249\*\*/, "atom"], [/Legacy\/preserved specification rows: \*\*479\*\*/, "spec"], [/Preserved union: \*\*728\/728\*\*/, "union"], [/Unclassified: \*\*0\*\*/, "unclassified"]]) if (!pattern.test(markdown)) throw new Error(`preservation map: stale ${label} total`);
  return { rows, legacyAtoms, specRows, unclassified, splits, introductions };
}
function safeCell(value) { return String(value).replace(/\|/g, "/").replace(/\r?\n/g, " ").replace(/\s+/g, " ").trim(); }
function pendingMarkdown(parsed, inventory) {
  const committed = parsed.flatMap((canon) => canon.capabilities.filter((row) => row.Scope === "COMMITTED").map((row) => ({ ...row, canon })));
  const exploratory = parsed.flatMap((canon) => canon.capabilities.filter((row) => row.Scope === "EXPLORATORY"));
  const competitiveOpen = committed.filter((row) => !competitivelyClosed(row));
  const lifecycleOpen = committed.filter((row) => !closed(row, row.canon.boundary));
  const lines = ["# Membrane pending capability work", "", "<!-- GENERATED by scripts/ci/check-atomic-canons.mjs --write. Do not hand-edit. -->", "", `Total capability rows: **${committed.length + exploratory.length}**`, `Committed capability atoms: **${committed.length}**`, `Exploratory capability rows: **${exploratory.length}**`, `Competitive current-best/closed: **${committed.length - competitiveOpen.length}**`, `Competitive pending: **${competitiveOpen.length}**`, `Lifecycle closure-proven: **${committed.length - lifecycleOpen.length}**`, `Lifecycle open/unproven: **${lifecycleOpen.length}**`, `Preserved legacy/spec rows: **${inventory.rows.length}**`, `Unclassified preserved rows: **${inventory.unclassified.length}**`, "", "Atomic & competitive state lives in `docs/canon/*.md`; comparison receipts live in `docs/provenance/foundation/2026-08-31-competitive-comparison/`; lifecycle qualification remains separate. This file is sole competitive pending-work index.", "", "## Canon summary", "", "| Subsystem | Boundary | Committed | Exploratory | Current best/closed | Competitive pending | Lifecycle closed | Groups | Implementations | Qualifications | Decisions |", "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"];
  for (const canon of parsed) {
    const capabilities = canon.capabilities.filter((row) => row.Scope === "COMMITTED"), canonCompetitiveOpen = capabilities.filter((row) => !competitivelyClosed(row)).length, canonLifecycleClosed = capabilities.filter((row) => closed(row, canon.boundary)).length;
    const canonExploratory = canon.capabilities.filter((row) => row.Scope === "EXPLORATORY").length;
    lines.push(`| [${canon.owner}](../canon/${canon.file}) | ${canon.boundary} | ${capabilities.length} | ${canonExploratory} | ${capabilities.length - canonCompetitiveOpen} | ${canonCompetitiveOpen} | ${canonLifecycleClosed} | ${canon.groups.length} | ${canon.implementations.length} | ${canon.qualifications.length} | ${canon.decisions.length} |`);
  }
  lines.push("", "## Exploratory capability rows", "", "Exploratory rows are discovered candidates, not committed product behavior.", "", "| Atom | Candidate behavior |", "|---|---|");
  for (const canon of parsed) {
    for (const row of canon.capabilities.filter((candidate) => candidate.Scope === "EXPLORATORY")) {
      lines.push(`| [${row.ID}](../canon/${canon.file}) | ${safeCell(row["Observable behavior"])} |`);
    }
  }
  lines.push("", "## Competitive pending capability atoms", "");
  for (const canon of parsed) {
    const rows = canon.capabilities.filter((row) => row.Scope === "COMMITTED" && !competitivelyClosed(row));
    if (!rows.length) continue;
    lines.push(`### ${canon.owner}`, "", "| Atom | Competitive state | Action | Deficit |", "|---|---|---|---|");
    for (const row of rows) lines.push(`| [${row.ID}](../canon/${canon.file}) | ${row.Competitive} | ${safeCell(row.Action)} | ${safeCell(`implementation=${row.Implementation}; verification=${row.Verification}; qualification=${row.Qualification}; delivery=${row.Delivery}/${canon.boundary}; evidence=${row.Evidence}`)} |`);
    lines.push("");
  }
  lines.push("## Preserved supporting specifications", "", "Supporting files retain detail; only this generated file indexes pending state.", "", "| Specification | Canon target |", "|---|---|", "| [Adapt harness efficiency](capabilities/adapt/harness-efficiency.md) | `ADP-036`, `ADP-038`, `ADP-040`, `ADP-043`–`ADP-071` |", "| [Blueprint findings lane](capabilities/blueprint/findings-lane.md) | `BPT-049`, `BPT-050`, `BPT-051`, `BPT-052`, `BPT-065`, `BPT-066`, `BPT-067` |", "| [Semantic context advisor](experiments/semantic-context-advisor.md) | `MEM-D003` |", "| [Membrane brand identity](design/membrane-brand-identity.md) | `MEM-D004` |", "| [Hub visual reference](design/hub/hub-mockup.html) | `MEM-D005` |", "", "## Unclassified preserved work", "", inventory.unclassified.length ? `${inventory.unclassified.length} rows require classification.` : "None.", "");
  return lines.join("\n");
}
function atomReadmeMarkdown(parsed, inventory) {
  const committed = parsed.reduce((sum, canon) => sum + canon.capabilities.filter((row) => row.Scope === "COMMITTED").length, 0);
  const exploratory = parsed.reduce((sum, canon) => sum + canon.capabilities.filter((row) => row.Scope === "EXPLORATORY").length, 0);
  const competitiveClosedCount = parsed.reduce((sum, canon) => sum + canon.capabilities.filter(competitivelyClosed).length, 0);
  const lifecycleClosedCount = parsed.reduce((sum, canon) => sum + canon.capabilities.filter((row) => closed(row, canon.boundary)).length, 0);
  const lines = ["# Membrane atomic capability canons", "", "<!-- GENERATED by scripts/ci/check-atomic-canons.mjs --write. Do not hand-edit. -->", "", "Each named subsystem owns one atomic canon. Competitive comparison & lifecycle qualification remain separate; only current-best committed atoms are competitively closed.", "", "## Current inventory", "", "| Canon | Boundary | Committed | Exploratory | Current best/closed | Competitive pending | Lifecycle closed |", "|---|---|---:|---:|---:|---:|---:|"];
  for (const canon of parsed) {
    const count = canon.capabilities.filter((row) => row.Scope === "COMMITTED").length, canonCompetitiveClosed = canon.capabilities.filter(competitivelyClosed).length, canonLifecycleClosed = canon.capabilities.filter((row) => closed(row, canon.boundary)).length;
    const canonExploratory = canon.capabilities.filter((row) => row.Scope === "EXPLORATORY").length;
    lines.push(`| [${canon.owner}](${canon.file}) | ${canon.boundary} | ${count} | ${canonExploratory} | ${canonCompetitiveClosed} | ${count - canonCompetitiveClosed} | ${canonLifecycleClosed} |`);
  }
  lines.push(`| **Total** | — | **${committed}** | **${exploratory}** | **${competitiveClosedCount}** | **${committed - competitiveClosedCount}** | **${lifecycleClosedCount}** |`, "", `Total capability rows: **${committed + exploratory}**`, "", "## Counting & closure", "", "Count only `COMMITTED` capability rows. Groups roll up children & never count. Implementation mechanisms, qualification gates & decisions support capabilities & never count independently.", "", "Competitive closure requires receipt-bound `CURRENT_BEST`; `DONOR_BETTER`, `CURRENT_INCOMPLETE`, & `UNRESOLVED` remain pending. Lifecycle closure remains independently derived from implementation, focused verification, qualification, delivery, & exact acceptance evidence.", "", "## Preservation", "", `Legacy atoms: **${inventory.legacyAtoms.length}**`, `Introduced atomic splits: **${inventory.splits.length}**`, `New capabilities after normalization: **${inventory.introductions.length}**`, `Legacy/specification rows: **${inventory.specRows.length}**`, `Preserved union: **${inventory.rows.length}/${inventory.rows.length}**`, `Unclassified: **${inventory.unclassified.length}**`, "", "See [preservation map](../provenance/migrations/2026-08-30-atomic-canons/preservation-map.md), [competitive comparison](../provenance/foundation/2026-08-31-competitive-comparison/README.md), & generated [pending index](../pending/README.md).", "", "## Register schemas", "", `Group: \`${headers.group.join(" | ")}\``, "", `Capability: \`${headers.capability.join(" | ")}\``, "", `Implementation: \`${headers.implementation.join(" | ")}\``, "", `Qualification: \`${headers.qualification.join(" | ")}\``, "", `Decision: \`${headers.decision.join(" | ")}\``, "");
  return lines.join("\n");
}
function validatePendingSupport(markdown) {
  const required = ["capabilities/adapt/harness-efficiency.md", "capabilities/blueprint/findings-lane.md", "experiments/semantic-context-advisor.md", "design/membrane-brand-identity.md", "design/hub/hub-mockup.html"];
  for (const relative of required) {
    if (!existsSync(path.join(root, "docs", "pending", relative))) throw new Error(`missing supporting specification ${relative}`);
    if (!markdown.includes(`](${relative})`)) throw new Error(`pending index omits ${relative}`);
  }
  const walk = (directory, prefix = "") => readdirSync(directory).flatMap((name) => {
    const absolute = path.join(directory, name), relative = prefix ? `${prefix}/${name}` : name;
    return statSync(absolute).isDirectory() ? walk(absolute, relative) : [relative.replace(/\\/g, "/")];
  });
  const observed = walk(path.join(root, "docs", "pending")).filter((file) => file !== "README.md" && /\.(?:md|html)$/.test(file)).sort();
  if (JSON.stringify(observed) !== JSON.stringify([...required].sort())) throw new Error(`pending supporting-document inventory differs: ${observed.join(", ")}`);
}

export const atomicCanonTestHooks = Object.freeze({ proofEvidence, comparisonEvidence, focusedProofLooksExact, closed, competitivelyClosed, similarity, parseCanon });
export function validateAtomicCanons({ write = false } = {}) {
  const canonFiles = readdirSync(atomDir).filter((file) => file.endsWith(".md") && file !== "README.md").sort();
  const expectedCanonFiles = canons.map((canon) => canon.file).sort();
  if (JSON.stringify(canonFiles) !== JSON.stringify(expectedCanonFiles)) throw new Error(`atomic canon inventory differs: ${canonFiles.join(", ")}`);
  const parsed = canons.map(parseCanon), { everyId, capabilityIds } = validateIdentity(parsed);
  validateReceiptReferences(parsed);
  validateComparisonReferences(parsed);
  validateSemanticOwnership(parsed);
  const inventory = validatePreservation(everyId, capabilityIds), expectedPending = pendingMarkdown(parsed, inventory), expectedReadme = atomReadmeMarkdown(parsed, inventory);
  validatePendingSupport(expectedPending);
  if (write) { writeFileSync(pendingPath, expectedPending, "utf8"); writeFileSync(atomReadmePath, expectedReadme, "utf8"); }
  else {
    if (readFileSync(pendingPath, "utf8").replace(/\r\n/g, "\n") !== expectedPending) throw new Error("docs/pending/README.md is stale; run checker with --write");
    if (readFileSync(atomReadmePath, "utf8").replace(/\r\n/g, "\n") !== expectedReadme) throw new Error("docs/canon/README.md is stale; run checker with --write");
  }
  const committed = parsed.flatMap((canon) => canon.capabilities.filter((row) => row.Scope === "COMMITTED").map((row) => ({ ...row, canon })));
  const lifecycleClosedRows = committed.filter((row) => closed(row, row.canon.boundary)), competitiveClosedRows = committed.filter(competitivelyClosed);
  return { canons: parsed.length, capabilityRows: parsed.reduce((sum, canon) => sum + canon.capabilities.length, 0), atoms: committed.length, exploratory: parsed.reduce((sum, canon) => sum + canon.capabilities.filter((row) => row.Scope === "EXPLORATORY").length, 0), competitiveClosed: competitiveClosedRows.length, competitiveOpen: committed.length - competitiveClosedRows.length, lifecycleClosed: lifecycleClosedRows.length, lifecycleOpen: committed.length - lifecycleClosedRows.length, groups: parsed.reduce((sum, canon) => sum + canon.groups.length, 0), implementations: parsed.reduce((sum, canon) => sum + canon.implementations.length, 0), qualifications: parsed.reduce((sum, canon) => sum + canon.qualifications.length, 0), decisions: parsed.reduce((sum, canon) => sum + canon.decisions.length, 0), preservationRows: inventory.rows.length, legacyAtoms: inventory.legacyAtoms.length, introducedSplits: inventory.splits.length, introducedCapabilities: inventory.introductions.length, specRows: inventory.specRows.length, unclassified: inventory.unclassified.length };
}

const invoked = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invoked) {
  try {
    const result = validateAtomicCanons({ write: process.argv.includes("--write") });
    console.log(`atomic canons PASS: ${result.canons} canons, ${result.capabilityRows} rows, ${result.atoms} committed, ${result.exploratory} exploratory, ${result.competitiveClosed} current-best/closed, ${result.competitiveOpen} competitive pending, ${result.lifecycleClosed} lifecycle-closed, ${result.preservationRows} preserved, ${result.unclassified} unclassified`);
  } catch (error) { console.error(`atomic canons FAIL: ${error.message}`); process.exitCode = 1; }
}
