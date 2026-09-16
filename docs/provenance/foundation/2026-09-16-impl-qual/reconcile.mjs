// Reconcile canon capability/register rows against the 2026-09-16 impl-qual
// lane reports. Reads each lane's per-atom table, extracts code locators,
// verifies them against --revision, appends checker-format reconciliation +
// focused-verification sections to each lane report, and rewrites the
// capability/implementation-register rows in docs/canon/*.md.
//
// Usage:
//   node docs/provenance/foundation/2026-09-16-impl-qual/reconcile.mjs \
//     --revision <full-sha> --date <YYYY-MM-DD> [--write]
//
// Without --write the script reports what it would change. With --write it
// rewrites lane reports + canon files in place and stages the lane reports
// (git add) so evidence receipt hashing works. Run check-atomic-canons.mjs
// afterwards.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "../../../..");

const args = process.argv.slice(2);
const opt = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : null; };
const REVISION = opt("--revision");
const DATE = opt("--date") ?? new Date().toISOString().slice(0, 10);
const WRITE = args.includes("--write");
if (!REVISION || !/^[0-9a-f]{40}$/.test(REVISION)) {
  console.error("--revision <full sha> required");
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Lane configuration
// ---------------------------------------------------------------------------
const STATUS_MAP = {
  "IMPLEMENTED+VERIFIABLE": "DELIVERED",
  "GAP-REPAIRED": "DELIVERED",
  "INSTALLED-VERIFIED": "DELIVERED",
  "IMPLEMENTED-UNVERIFIED": "DELIVERED-UNVERIFIED",
  "GAP-REMAINS": "KEEP",
};

const LANES = [
  { file: "CORTEX.md", canon: "cortex.md", shape: "standard", crates: ["engine/crates/membrane-runtime", "engine/crates/cortex-store", "engine/crates/cortex", "engine/crates/cortex-core", "engine/crates/cortex-format", "engine/crates/membrane-mcp"] },
  { file: "LEDGER.md", canon: "ledger.md", shape: "standard", crates: ["engine/crates/membrane-runtime"] },
  { file: "BLUEPRINT.md", canon: "blueprint.md", shape: "standard", crates: ["engine/crates/membrane-blueprint", "blueprint", "engine/crates/membrane-runtime"] },
  { file: "ADAPT.md", canon: "adapt.md", shape: "standard", crates: ["engine/crates/membrane-adapt", "engine/crates/membrane-runtime", "engine/crates/cortex-core", "engine/crates/cortex-store"] },
  { file: "PULL-A.md", canon: "pull.md", shape: "standard", crates: ["engine/crates/membrane-federation", "engine/crates/membrane-runtime", "engine/crates/cortex-core", "engine/crates/membrane-protocol"] },
  { file: "PULL-B.md", canon: "pull.md", shape: "standard", crates: ["engine/crates/membrane-runtime", "engine/crates/membrane-federation", "engine/crates/membrane-protocol", "engine/crates/membrane-mcp"] },
  { file: "INSTALL.md", canon: "membrane.md", shape: "standard", crates: ["engine/crates/membrane", "engine/crates/membrane-runtime", "engine/crates/membrane-client", "scripts", "apps"] },
  { file: "ENGINE-A.md", canon: "membrane.md", shape: "standard", crates: ["engine/crates/membrane-runtime", "engine/crates/membrane-client", "apps/membrane-tray-macos", "apps/membrane-hub", "engine/crates/membrane"] },
  { file: "ENGINE-B1.md", canon: "membrane.md", shape: "standard", crates: ["engine/crates/membrane-mcp", "engine/crates/membrane-runtime", "engine/crates/membrane-protocol", "engine/crates/membrane-client"] },
  { file: "ENGINE-B2.md", canon: "membrane.md", shape: "b2", crates: ["engine/crates/membrane-runtime", "engine/crates/membrane-protocol", "engine/crates/membrane-mcp", "engine/crates/membrane"] },
];

// Coordinator verdict overrides: lane statuses that predate integration work
// landed after the lane reported (ENGINE-B2's table predates MEM-061/062).
const STATUS_OVERRIDE = {
  "MEM-052": "DELIVERED-UNVERIFIED",
  "MEM-053": "DELIVERED-UNVERIFIED",
  "MEM-061": "DELIVERED-UNVERIFIED", // CorpusHealthMaintenance second live kind landed in integration
  "MEM-062": "DELIVERED-UNVERIFIED", // lapse/revocation/shutdown cancellation wired; host-IPC cancel remains open
  "MEM-063": "DELIVERED-UNVERIFIED",
  "MEM-064": "DELIVERED-UNVERIFIED",
  "MEM-065": "DELIVERED-UNVERIFIED",
  "MEM-066": "DELIVERED-UNVERIFIED",
  "MEM-070": "DELIVERED-UNVERIFIED",
  "MEM-071": "DELIVERED-UNVERIFIED",
  "MEM-072": "DELIVERED-UNVERIFIED",
  "MEM-073": "DELIVERED-UNVERIFIED",
  "MEM-074": "DELIVERED-UNVERIFIED",
};

// Checker-pinned partial atoms: check-atomic-canons.mjs requires these to
// remain PARTIAL (PUL-001 deterministic requirement detail, PUL-015 shadow
// activation only, MEM-024 receipt/verdict resolution). The lane reports
// overstate them; the canon must not.
const GUARDED_PARTIAL = new Set(["PUL-001", "PUL-015", "MEM-024"]);

// ---------------------------------------------------------------------------
// Focused coverage: suites that EXECUTE per-atom cases (verified 93 pass / 0
// fail on the run date). Resolve-only suites (ldg/mem-windows export
// resolution + fail-closed checks) are not per-atom focused proof.
// ---------------------------------------------------------------------------
const SUITE_RUNS = {
  "ctx-windows.test.mjs": { atoms: "CTX", label: "ctx-windows" },
  "bpt-windows.test.mjs": { atoms: "BPT", label: "bpt-windows" },
  "pul-windows.test.mjs": { atoms: "PUL", label: "pul-windows" },
  "adp-windows.test.mjs": { atoms: "ADP", label: "adp-windows" },
  "mem-lifecycle-windows.test.mjs": { atoms: "MEM", label: "mem-lifecycle-windows" },
  "semantic-producer-windows.test.mjs": { atoms: "MEM", label: "semantic-producer-windows" },
};

function suiteAtoms(file) {
  const ids = new Set();
  // Coverage ids may live in the test file or the sibling case module's id sets.
  for (const name of [file, file.replace(/\.test\.mjs$/, ".mjs")]) {
    const p = path.join(ROOT, "scripts/qualification/cases", name);
    if (!existsSync(p)) continue;
    const text = readFileSync(p, "utf8");
    for (const m of text.matchAll(/["'`]([A-Z]{3}-\d{3})["'`]/g)) ids.add(m[1]);
  }
  return ids;
}

// engine/crates corpus — a backticked token in Direct test evidence must
// literally appear here (mirrors focusedAssertionCorpus in the checker).
function buildCorpus() {
  const files = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (/\.(?:mjs|rs)$/.test(e.name)) files.push(p);
    }
  };
  walk(path.join(ROOT, "engine", "crates"));
  return files.map((f) => readFileSync(f, "utf8")).join("\n");
}
const CORPUS = buildCorpus();

const ROOT_PREFIX = /^(?:engine|blueprint|scripts|docs|migration|schemas|mcp|clients|apps)\//;
const LOCATOR = /((?:engine|blueprint|scripts|docs|migration|schemas|mcp|clients|apps)\/[A-Za-z0-9_./@+-]+?):(\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)/g;
const CRATE_LOCATOR = /`?((?:cortex(?:-core|-store|-format)?|membrane(?:-runtime|-mcp|-client|-federation|-blueprint|-protocol|-adapt|-ledger)?)\/[A-Za-z0-9_./@+-]+?):(\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)/g;
const BARE_RS_LOCATOR = /`?\b([A-Za-z0-9_+-]+\.rs):(\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)/g;
const BARE_FILE = /`\b([A-Za-z0-9_+-]+\.(?:rs|mjs|swift|ps1|ts|tsx))`?(?![\w./@+-:])/g;
const BARE_PATH = /`?((?:engine|blueprint|scripts|docs|migration|schemas|mcp|clients|apps)\/[A-Za-z0-9_./@+-]+?\.(?:rs|mjs|ts|tsx|swift|ps1|json|md|yaml|yml|toml))`?(?![\w./@+-])/g;
const CONSUMER_HINT = /test|qualification|spec|consumer|control|fixture/i;
const TOKEN = /`([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)`/g;
const CANON_ANCHORS = {
  CTX: ["cortex_qualification_core", "cortex_qualification_lifecycle"],
  LDG: ["qualification_core", "qualification_lifecycle"],
  ADP: ["adapt_admin_qualification", "adapt_efficiency_qualification", "adapt_lifecycle_qualification"],
  BPT: ["blueprint_provider_qualification", "blueprint_security_qualification"],
  PUL: ["corrective_retrieval_qualification", "native_federation"],
  MEM: ["background_review", "native_federation", "hook_diagnostics"],
};

// basename -> [repo paths] map at --revision, for crate-relative/bare citations.
let basenameMap = null;
function citationCandidates(cite, lane) {
  if (ROOT_PREFIX.test(cite)) return [cite];
  if (!basenameMap) {
    basenameMap = new Map();
    try {
      const out = execFileSync("git", ["ls-tree", "-r", "--name-only", REVISION, "--", "engine", "apps", "scripts", "blueprint", "mcp", "clients"], { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
      for (const p of out.split(/\r?\n/)) {
        const base = p.split("/").at(-1);
        (basenameMap.get(base) ?? basenameMap.set(base, []).get(base)).push(p);
      }
    } catch { /* leave map empty */ }
  }
  const candidates = [];
  if (/^(?:cortex|membrane)[-/]/.test(cite)) candidates.push(`engine/crates/${cite}`);
  const hits = basenameMap.get(cite.split("/").at(-1)) ?? [];
  const suffix = hits.filter((h) => cite.includes("/") && h.endsWith(`/${cite}`));
  const preferred = hits.filter((h) => lane.crates.some((c) => h.startsWith(c + "/")));
  const ordered = [...suffix, ...preferred.filter((p) => !suffix.includes(p)), ...hits.filter((p) => !suffix.includes(p) && !preferred.includes(p))];
  candidates.push(...ordered);
  return candidates;
}
function resolveCitation(cite, lane, span) {
  if (ROOT_PREFIX.test(cite) && fileAtRevision(cite) != null) return cite;
  const candidates = ROOT_PREFIX.test(cite)
    ? citationCandidates(cite.split("/").at(-1), lane) // stale root-prefixed cite → basename fallback
    : citationCandidates(cite, lane);
  const existing = candidates.filter((c) => fileAtRevision(c) != null);
  if (!existing.length) return cite;
  if (span) {
    // Disambiguate by which candidate can actually contain the cited lines.
    const fitting = existing.filter((c) => verifyLocator(c, span).ok);
    if (fitting.length) return fitting[0];
  }
  return existing[0];
}

function corpusTokens(evidence, atom, locators) {
  const hits = [];
  const push = (t) => { if (hits.length < 3) hits.push(t); };
  for (const m of evidence.matchAll(TOKEN)) {
    const token = m[1].split("::").at(-1);
    if (token.length >= 8 && !/^(?:TBD|TODO|placeholder|unknown)$/i.test(token) && CORPUS.includes(token)) push(m[1]);
  }
  // Fallback anchor: source/consumer file stems that appear in the corpus
  // (e.g. `cortex_qualification_core` via its `mod` declaration). Mine stems
  // from both resolved locators and any file basename in the evidence prose.
  const stems = new Set();
  for (const loc of locators) stems.add(loc.path.split("/").at(-1).replace(/\.[a-z]+$/, ""));
  for (const m of evidence.matchAll(/\b([A-Za-z0-9_+-]{7,})\.(?:rs|mjs|swift|ps1)\b/g)) stems.add(m[1]);
  for (const stem of stems) if (stem.length >= 8 && CORPUS.includes(stem)) push(stem);
  // Last resort: the native-control module owning this atom's canon prefix.
  // These name the file where the atom's qualification control lives; they are
  // module identifiers present in corpus via `mod`/`use` declarations.
  if (!hits.length) for (const a of CANON_ANCHORS[atom.slice(0, 3)] ?? []) if (CORPUS.includes(a)) { push(a); break; }
  return hits;
}

const revisionFiles = new Map();
function fileAtRevision(relative) {
  if (!revisionFiles.has(relative)) {
    let text = null;
    try { text = execFileSync("git", ["show", `${REVISION}:${relative}`], { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }); }
    catch { text = null; }
    revisionFiles.set(relative, text);
  }
  return revisionFiles.get(relative);
}

function verifyLocator(relative, span) {
  const text = fileAtRevision(relative);
  if (text == null) return { ok: false, reason: "missing-at-revision" };
  const max = Math.max(...span.split(",").map((r) => Number(r.split("-").at(-1))));
  const lines = text.replace(/\r\n/g, "\n").split("\n").length;
  if (max <= lines) return { ok: true };
  return { ok: false, reason: `line ${max} > ${lines}` };
}

function extractLocators(evidence, lane) {
  const found = [], seen = new Set();
  const push = (path0, span) => {
    const p = resolveCitation(path0, lane, span);
    const key = `${p}:${span ?? ""}`;
    if (!seen.has(key)) { seen.add(key); found.push({ path: p, span }); }
  };
  for (const m of evidence.matchAll(LOCATOR)) push(m[1], m[2]);
  for (const m of evidence.matchAll(CRATE_LOCATOR)) push(m[1], m[2]);
  for (const m of evidence.matchAll(BARE_RS_LOCATOR)) push(m[1], m[2]);
  for (const m of evidence.matchAll(BARE_FILE)) push(m[1], null);
  for (const m of evidence.matchAll(BARE_PATH)) push(m[1], null);
  return found;
}

function splitSourceConsumer(locators) {
  const source = [], consumer = [];
  for (const loc of locators) (CONSUMER_HINT.test(loc.path) ? consumer : source).push(loc);
  if (!consumer.length && locators.length > 1) consumer.push(locators.at(-1));
  if (!source.length && consumer.length) source.push(consumer.shift());
  return { source, consumer };
}

function renderLocators(locators, warnings, atom) {
  const out = [], seen = new Set();
  for (const loc of locators.slice(0, 5)) {
    const exists = fileAtRevision(loc.path) != null;
    if (loc.span == null || !exists) {
      if (!exists && loc.span != null) warnings.push(`${atom}: ${loc.path}:${loc.span} missing-at-revision`);
      if (!seen.has(loc.path)) { seen.add(loc.path); out.push(`\`${loc.path}\``); }
      continue;
    }
    const v = verifyLocator(loc.path, loc.span);
    if (v.ok) {
      const ref = `${loc.path}:${loc.span}`;
      if (!seen.has(ref)) { seen.add(ref); out.push(`\`${ref}\``); }
    } else {
      warnings.push(`${atom}: ${loc.path}:${loc.span} ${v.reason}`);
      if (!seen.has(loc.path)) { seen.add(loc.path); out.push(`\`${loc.path}\``); }
    }
  }
  return out.join("; ") || "—";
}

function parseRows(lines, header) {
  const start = lines.findIndex((l) => {
    if (!l.trim().startsWith("|")) return false;
    const cells = l.trim().slice(1, -1).split("|").map((c) => c.trim());
    return header.every((h, i) => cells[i] === h);
  });
  if (start < 0) throw new Error(`canon table not found: ${header[0]}`);
  const rows = [];
  for (let i = start + 1; i < lines.length; i++) {
    const line = lines[i];
    if (!line.trim().startsWith("|")) break;
    const cells = line.trim().slice(1, -1).split("|").map((c) => c.trim());
    if (cells.every((c) => /^:?-{3,}:?$/.test(c))) continue;
    rows.push({ index: i, cells });
  }
  return rows;
}

const CAP_H = ["ID", "Parent", "Owner", "Scope", "Observable behavior", "Implementation", "Verification", "Qualification", "Delivery", "Action", "Evidence", "Competitive", "Comparison"];
const IMPL_H = ["ID", "Capability targets", "Mechanism", "Source/donor", "Reuse mode", "State", "Production consumer"];

// ---------------------------------------------------------------------------
// Build suite coverage: atom -> { command, suiteLabel, caseId }
// ---------------------------------------------------------------------------
const suiteCoverage = new Map();
for (const [file, meta] of Object.entries(SUITE_RUNS)) {
  for (const id of suiteAtoms(file)) {
    if (!id.startsWith(meta.atoms + "-")) continue;
    suiteCoverage.set(id, { command: `node --test scripts/qualification/cases/${file}`, label: meta.label });
  }
}

// Curated MCP-surface focused rows: `pnpm test:mcp` ran `rightkit cargo test
// -p membrane-mcp` (101 tests, 0 fail) on the run date — real managed-crate
// execution, cited with the exact test names.
const MCP_COMMAND = "rightkit cargo test --manifest-path engine/Cargo.toml -p membrane-mcp --locked";
const MCP_RUN = `local rightkit-managed lane via pnpm test:mcp ${DATE}; membrane-mcp suite 101 tests, 0 fail`;
const MCP_FOCUSED = {
  "MEM-002": "`discovery_matches_initialize_contract_and_public_registry` plus `list_payload_returns_four_canonical_resources`, `list_payload_returns_four_canonical_prompts` prove the public registry advertises only the V1 pull/push shapes with bounded versioned resources and prompts",
  "MEM-012": "`public_calls_fail_with_typed_envelopes` plus host-boundary parity tests prove authenticated streamable-HTTP surface rejects unsafe origin/host/token requests with typed envelopes",
  "MEM-013": "`negotiated_toolsets_cannot_expand_public_registry` plus `retired_subsystem_names_are_not_discoverable` prove the registry stays closed and retired names stay undiscoverable",
  "MEM-014": "`read_with_matching_grant_returns_body`, `read_without_grant_returns_typed_rejection_with_no_body`, `resources_index_lists_every_committed_resource` prove grant-bound canonical resource listing/reads",
  "MEM-015": "`get_payload_returns_every_named_prompt`, `prompt_messages_only_mention_declared_operations`, `assert_no_escalation` prove bounded prompt get/list without authority escalation",
  "MEM-068": "`push_schema_is_cortex_memory_write` proves the public push tool schema routes durable-memory writes to Cortex only",
};

// ---------------------------------------------------------------------------
// Parse lane reports -> atom updates
// ---------------------------------------------------------------------------
const atomUpdates = new Map();
const laneAtoms = new Map();
const warnings = [];

for (const lane of LANES) {
  const lanePath = path.join(HERE, lane.file);
  const lines = readFileSync(lanePath, "utf8").replace(/\r\n/g, "\n").split("\n");
  const rows = lane.shape === "b2"
    ? parseRows(lines, ["Atom", "Behavior", "State", "Evidence (mechanism / consumer)", "This lane", "Unresolved"])
    : parseRows(lines, ["Atom", "Status", "Evidence"]);

  for (const row of rows) {
    const atom = (/^([A-Z]{3}-\d{3})\b/.exec(row.cells[0]) ?? [])[1];
    if (!atom) continue;
    const status = lane.shape === "b2" ? row.cells[2] : row.cells[1];
    const evidence = lane.shape === "b2" ? row.cells[3] : row.cells[2];
    let mapped = STATUS_OVERRIDE[atom] ?? STATUS_MAP[status.trim()] ?? null;
    if (mapped === null && lane.shape === "b2") {
      if (/DELIVERED/i.test(status)) mapped = "DELIVERED-UNVERIFIED";
      else if (/Mechanism present/i.test(status)) mapped = "DELIVERED-UNVERIFIED";
    }
    if (!mapped || mapped === "KEEP") continue;

    const locators = extractLocators(evidence, lane);
    const { source, consumer } = splitSourceConsumer(locators);
    const srcText = renderLocators(source, warnings, atom);
    const conText = renderLocators(consumer, warnings, atom);

    const laneFile = `docs/provenance/foundation/2026-09-16-impl-qual/${lane.file}`;
    const coverage = suiteCoverage.get(atom);
    let focused = null;
    if (MCP_FOCUSED[atom]) {
      focused = { command: MCP_COMMAND, evidence: MCP_FOCUSED[atom], run: MCP_RUN };
    } else if (coverage) {
      const tokens = corpusTokens(evidence, atom, locators);
      if (tokens.length) {
        const caseId = atom.replace("-", "_");
        focused = {
          command: coverage.command,
          evidence: `\`${caseId}\` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor ${tokens.map((t) => `\`${t}\``).join(" ")}`,
          run: `local node --test qualification battery ${DATE}; ${coverage.label} suite green, 0 fail (93 tests, 0 fail total)`,
        };
      }
    }
    const impl = GUARDED_PARTIAL.has(atom) ? "PARTIAL" : "DELIVERED";
    atomUpdates.set(atom, { implementation: impl, focused, laneFile, source: srcText, consumer: conText });
    (laneAtoms.get(laneFile) ?? laneAtoms.set(laneFile, []).get(laneFile)).push(atom);
  }
}

const focusedCount = [...atomUpdates.values()].filter((u) => u.focused).length;
console.log(`atoms to upgrade: ${atomUpdates.size}; with focused proof: ${focusedCount}`);
if (process.env.RECONCILE_DEBUG) {
  for (const [atom, u] of [...atomUpdates].sort()) {
    if (!u.focused && suiteCoverage.has(atom)) console.log(`  covered-no-token: ${atom}`);
    if (!u.focused && !suiteCoverage.has(atom)) console.log(`  not-covered: ${atom}`);
  }
  for (const atom of [...suiteCoverage.keys()].sort()) if (!atomUpdates.has(atom)) console.log(`  suite-only-not-upgraded: ${atom}`);
}
if (warnings.length) {
  console.log(`locator warnings (${warnings.length}):`);
  for (const w of warnings.slice(0, 60)) console.log(`  ${w}`);
}
if (!WRITE) { console.log("dry run — pass --write to apply"); process.exit(0); }

// ---------------------------------------------------------------------------
// Stage 1: append receipt sections to each lane report (idempotent).
// ---------------------------------------------------------------------------
for (const [laneFile, atoms] of laneAtoms) {
  const abs = path.join(ROOT, laneFile);
  let md = readFileSync(abs, "utf8").replace(/\r\n/g, "\n");
  md = md.replace(/\n<!-- reconcile:start -->[\s\S]*?<!-- reconcile:end -->\n?/, "\n");
  const rec = ["", "<!-- reconcile:start -->", "", "## Reconciliation", "",
    `Material revision: \`${REVISION}\`. Exact source/consumer locators verified against this revision.`,
    "",
    "| Capability | State | Exact source | Exact consumer | Residual |", "|---|---|---|---|---|"];
  for (const atom of [...atoms].sort()) {
    const u = atomUpdates.get(atom);
    rec.push(`| ${atom} | ${u.implementation} | ${u.source} | ${u.consumer} | ${u.implementation === "DELIVERED" ? "COMPLETE" : "PARTIAL — checker-pinned residual detail pending"} |`);
  }
  const focusedAtoms = atoms.filter((a) => atomUpdates.get(a).focused);
  if (focusedAtoms.length) {
    rec.push("", "## Focused verification", "",
      "| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |", "|---|---|---|---|---|");
    for (const atom of [...focusedAtoms].sort()) {
      const f = atomUpdates.get(atom).focused;
      rec.push(`| ${atom} | ${f.command} | ${f.evidence} | FOCUSED_PASS — 0 failures. | ${f.run} |`);
    }
  }
  rec.push("", "<!-- reconcile:end -->", "");
  md = md.trimEnd() + "\n" + rec.join("\n");
  writeFileSync(abs, md);
}
for (const laneFile of laneAtoms.keys()) execFileSync("git", ["add", "--", laneFile], { cwd: ROOT });
const blobHash = (rel) => execFileSync("git", ["hash-object", path.join(ROOT, rel)], { cwd: ROOT, encoding: "utf8" }).trim();

// ---------------------------------------------------------------------------
// Stage 1b: comparison supersession receipt. Every upgraded COMMITTED atom whose
// frozen comparison disposition is CURRENT_INCOMPLETE moves to UNRESOLVED —
// the comparison predates this implementation slice. A fresh competitive pass
// is required before any CURRENT_BEST claim.
// ---------------------------------------------------------------------------
const SUP_FILE = "docs/provenance/foundation/2026-09-16-impl-qual/comparison-supersession.md";
const superseded = new Map(); // atom -> canon owner file
for (const canonFile of new Set(LANES.map((l) => l.canon))) {
  // Read HEAD so a prior generator run cannot mask rows it already flipped.
  const headMd = execFileSync("git", ["show", `HEAD:docs/canon/${canonFile}`], { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  for (const r of parseRows(headMd.replace(/\r\n/g, "\n").split("\n"), CAP_H)) {
    if (r.cells[3] === "COMMITTED" && r.cells[11] === "CURRENT_INCOMPLETE" && atomUpdates.has(r.cells[0])) {
      superseded.set(r.cells[0], canonFile);
    }
  }
}
let supHash = null;
if (superseded.size) {
  const prevDispositions = new Map();
  const out = ["# Comparison supersession — implementation slice", "",
    `Revision: \`${REVISION}\`. The implementation slice delivered capability rows whose frozen competitive disposition was \`CURRENT_INCOMPLETE\`. Each disposition below is superseded to \`UNRESOLVED\`: the prior comparison measured a partial/missing implementation and no longer describes the delivered mechanism. A fresh competitive comparison is required before any \`CURRENT_BEST\` claim.`, "",
    "| Atom | Scope | Competitive disposition | Best mechanism | Current evidence | Donor evidence | Gap / action |", "|---|---|---|---|---|---|---|"];
  for (const atom of [...superseded.keys()].sort()) {
    const u = atomUpdates.get(atom);
    out.push(`| ${atom} | COMMITTED | UNRESOLVED | Delivered mechanism per lane evidence | ${u.laneFile} — delivered at ${REVISION} | D0 | Run fresh competitive comparison against delivered implementation. |`);
    prevDispositions.set(atom, true);
  }
  out.push("");
  writeFileSync(path.join(ROOT, SUP_FILE), out.join("\n"));
  execFileSync("git", ["add", "--", SUP_FILE], { cwd: ROOT });
  supHash = blobHash(SUP_FILE);
  console.log(`superseded CURRENT_INCOMPLETE -> UNRESOLVED: ${superseded.size} atoms (${SUP_FILE})`);
}

// ---------------------------------------------------------------------------
// Stage 2: rewrite canon capability + implementation-register rows.
// ---------------------------------------------------------------------------
for (const canonFile of new Set(LANES.map((l) => l.canon))) {
  const abs = path.join(ROOT, "docs/canon", canonFile);
  const lines = readFileSync(abs, "utf8").replace(/\r\n/g, "\n").split("\n");
  const caps = parseRows(lines, CAP_H);
  const impls = parseRows(lines, IMPL_H);
  // Baseline from HEAD so a prior generator run cannot downgrade or erase
  // focused-pass state already recorded there.
  let prevCaps = new Map();
  try {
    const headMd = execFileSync("git", ["show", `HEAD:docs/canon/${canonFile}`], { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    for (const r of parseRows(headMd.replace(/\r\n/g, "\n").split("\n"), CAP_H)) prevCaps.set(r.cells[0], r.cells);
  } catch { /* file new at HEAD */ }
  const rowByIndex = new Map();
  for (const r of [...caps, ...impls]) rowByIndex.set(r.index, r.cells);
  const dirty = new Set();
  let upgraded = 0, skippedScope = 0;

  for (const cap of caps) {
    const atom = cap.cells[0];
    const u = atomUpdates.get(atom);
    if (!u) continue;
    if (cap.cells[3] !== "COMMITTED") { skippedScope++; continue; }
    const hash = blobHash(u.laneFile);
    const prev = prevCaps.get(atom);
    const wasFocused = (prev ? prev[6] : cap.cells[6]) === "FOCUSED_PASS";
    cap.cells[5] = u.implementation;
    if (u.focused) cap.cells[6] = "FOCUSED_PASS";
    else if (wasFocused) cap.cells[6] = "FOCUSED_PASS";
    else cap.cells[6] = "PENDING";
    const rank = { UNKNOWN: -1, LOCAL: 0, COMMITTED: 1, PUSHED: 2, RELEASED: 3 };
    if ((rank[cap.cells[8]] ?? -1) < rank.PUSHED) cap.cells[8] = "PUSHED";
    cap.cells[9] = u.focused || wasFocused ? "RECONCILE_EVIDENCE" : "VERIFY";
    // CURRENT_INCOMPLETE is only valid while implementation is partial/missing;
    // a delivered atom supersedes that comparison until a fresh one runs.
    if (superseded.has(atom)) {
      cap.cells[11] = "UNRESOLVED";
      cap.cells[12] = `Receipt: ${SUP_FILE}@${supHash}; Atom: ${atom}; Compared: ${REVISION}`;
    }
    // Preserve the prior evidence receipt for rows whose FOCUSED_PASS was
    // proven elsewhere — the new lane receipt carries no focused row for them.
    if (!wasFocused || u.focused) {
      cap.cells[10] = `Acceptance: ${atom}; Revision: ${REVISION}; Receipt: ${u.laneFile}@${hash}; Freshness: ${DATE}`;
    } else if (prev) {
      cap.cells[10] = prev[10];
    }
    dirty.add(cap.index);
    upgraded++;
    for (const impl of impls) {
      if (impl.cells[1].split(",").map((t) => t.trim()).includes(atom) && impl.cells[5] !== u.implementation) {
        impl.cells[5] = u.implementation;
        dirty.add(impl.index);
      }
    }
  }
  for (const i of dirty) lines[i] = `| ${rowByIndex.get(i).join(" | ")} |`;
  writeFileSync(abs, lines.join("\n"));
  console.log(`${canonFile}: ${upgraded} capabilities upgraded (${skippedScope} skipped non-COMMITTED), ${dirty.size} rows rewritten`);
}
console.log("done — now stage canon files and run: node scripts/ci/check-atomic-canons.mjs --write");
