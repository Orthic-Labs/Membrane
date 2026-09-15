// Generates the required-atom coverage matrix from the six committed canons.
// Usage: node docs/provenance/foundation/2026-09-16-coverage-matrix/generate.mjs
// Outputs matrix.md + atoms.json beside this script. Read-only against canons.
import { readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..", "..", "..");
const CANONS = ["membrane", "pull", "cortex", "blueprint", "ledger", "adapt"];

function cells(line) {
  return line.trim().slice(1, -1).split("|").map((cell) => cell.trim());
}
function tableAfter(markdown, heading) {
  const lines = markdown.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === heading);
  if (start < 0) return [];
  const rows = [];
  for (let i = start + 1; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line.startsWith("|")) { if (rows.length) break; continue; }
    rows.push(cells(line));
  }
  return rows.filter((row) => !row.every((cell) => /^:?-{3,}:?$/.test(cell)));
}
function records(markdown, heading) {
  const rows = tableAfter(markdown, heading);
  if (!rows.length) return [];
  const header = rows[0];
  return rows.slice(1).map((row) => Object.fromEntries(header.map((key, i) => [key, row[i] ?? ""])));
}
function targets(value) {
  return !value || value === "—" ? [] : value.split(",").map((t) => t.trim()).filter(Boolean);
}
function depsOf(impl) {
  if (!impl) return "—";
  const m = /dependencies:\s*([^;]+)/.exec(impl["Source/donor"] || "");
  return m ? m[1].trim() : "—";
}
function firstReceipt(evidence) {
  if (!evidence || evidence === "PENDING") return "PENDING";
  const m = /Receipt:\s*([^;\s]+)/.exec(evidence);
  return m ? m[1] : evidence.slice(0, 80);
}

// MEM partition decided by coordinator (Legion) 2026-09-16:
// INSTALL owns installation/update/rollback transactions, host plugin
// projections, SessionStart & per-host proof. ENGINE owns everything else:
// planner, authz, identity, MCP semantics, receipts, scheduling, lifecycle.
const MEM_INSTALL = new Set([
  "MEM-018", "MEM-019", "MEM-020", "MEM-021",
  "MEM-045", "MEM-046", "MEM-047", "MEM-048",
  "MEM-057", "MEM-058", "MEM-059", "MEM-060",
  "MEM-067", "MEM-069",
]);
const LANE = { membrane: "ENGINE", pull: "PULL", cortex: "CORTEX", blueprint: "BLUEPRINT", ledger: "LEDGER", adapt: "ADAPT" };
const laneOf = (canon, id) => (canon === "membrane" && MEM_INSTALL.has(id) ? "INSTALL" : LANE[canon]);

const MILESTONE = [
  { name: "M1 installed-vertical", re: /^(MEM-006|MEM-007|MEM-008|MEM-009|MEM-010|MEM-011|MEM-012|MEM-013|MEM-014|MEM-015|MEM-016|MEM-017|MEM-045|MEM-046|MEM-047|MEM-055|MEM-056|MEM-067|MEM-068|MEM-069|PUL-011|PUL-012|PUL-015|PUL-027|PUL-035|PUL-042|PUL-043|PUL-050|CTX-043|LDG-022)$/ },
  { name: "M2 source/storage", re: /^(BPT-021|BPT-056|BPT-072)/ },
  { name: "M3 hosts/updates", re: /^(MEM-018|MEM-019|MEM-020|MEM-021|MEM-031|MEM-045|MEM-046|MEM-047|MEM-048|MEM-049|MEM-050|MEM-054|MEM-057|MEM-058|MEM-059|MEM-060|MEM-067|MEM-069|BPT-058|BPT-061|BPT-062|BPT-063)/ },
  { name: "M4 adapt-loop", re: /^(ADP-003|ADP-004|ADP-005|ADP-006|ADP-007|ADP-008|ADP-009|ADP-010|ADP-011|ADP-012|ADP-013|ADP-014|ADP-015|ADP-016|ADP-017|ADP-018|ADP-019|ADP-023|ADP-024|ADP-025|ADP-026|ADP-027|ADP-028|ADP-029|ADP-030|ADP-034|ADP-035|ADP-036|ADP-038|ADP-040|ADP-042|ADP-072|ADP-073|ADP-074|ADP-075|PUL-011)/ },
];
const milestoneOf = (id) => MILESTONE.filter((m) => m.re.test(id)).map((m) => m.name).join("+") || "M5 remaining";

const atoms = [];
for (const canon of CANONS) {
  const md = readFileSync(join(ROOT, "docs", "canon", `${canon}.md`), "utf8");
  const caps = records(md, "## Capability ledger").filter((r) => r.Scope === "COMMITTED");
  const impls = records(md, "## Implementation register");
  const quals = records(md, "## Qualification ledger");
  const implFor = (id) => impls.find((r) => targets(r["Capability targets"]).includes(id) && r["Reuse mode"] !== "RECLASSIFIED_IMPLEMENTATION");
  const qualFor = (id) => quals.find((r) => targets(r["Capability targets"]).includes(id) && r.State !== "NOT_REQUIRED");
  for (const cap of caps) {
    const impl = implFor(cap.ID);
    const qual = qualFor(cap.ID);
    atoms.push({
      canon,
      id: cap.ID,
      lane: laneOf(canon, cap.ID),
      milestone: milestoneOf(cap.ID),
      behavior: cap["Observable behavior"],
      implState: cap.Implementation || "—",
      verification: cap.Verification || "—",
      qualification: cap.Qualification || "—",
      delivery: cap.Delivery || "—",
      action: cap.Action || "—",
      competitive: cap.Competitive || "—",
      mechanism: impl?.Mechanism || "—",
      productionConsumer: impl?.["Production consumer"] || "—",
      implRowState: impl?.State || "—",
      dependencies: depsOf(impl),
      acceptanceBoundary: qual?.["Acceptance boundary"] || "—",
      qualState: qual?.State || "—",
      qualEvidence: qual?.Evidence || "—",
      materialRevision: qual?.["Material revision"] || "—",
      evidence: firstReceipt(cap.Evidence),
    });
  }
}

atoms.sort((a, b) => a.lane.localeCompare(b.lane) || a.id.localeCompare(b.id));

const laneCounts = {};
for (const a of atoms) laneCounts[a.lane] = (laneCounts[a.lane] || 0) + 1;

const md = [];
md.push("# Membrane required-atom coverage matrix — 2026-09-16");
md.push("");
const gitSha = execSync("git rev-parse HEAD", { cwd: ROOT, encoding: "utf8" }).trim();
md.push(`Generated by \`generate.mjs\` from committed canon rows at checkout \`${gitSha}\`.`);
md.push("Boundary for every row: **RELEASED**. Owner lane assigns exactly one accountable implementer;");
md.push("shared flows may supply evidence to several owners. Competitive state is historical context,");
md.push("never lifecycle scope.");
md.push("");
md.push("## Lane totals");
md.push("");
md.push("| Lane | Atoms |");
md.push("|---|---:|");
for (const [lane, count] of Object.entries(laneCounts).sort()) md.push(`| ${lane} | ${count} |`);
md.push(`| **Total** | **${atoms.length}** |`);
md.push("");
md.push("## Matrix");
md.push("");
md.push("| Atom | Owner lane | Production entry → state owner → consumer | Existing proof | Required acceptance | Dependencies | Current gap | Evidence location |");
md.push("|---|---|---|---|---|---|---|---|");
for (const a of atoms) {
  const prod = `\`${a.mechanism}\` → ${a.canon} → \`${a.productionConsumer}\``.replace(/\|/g, "\\|");
  const proof = `impl=${a.implState}/${a.implRowState}; verif=${a.verification}; qual=${a.qualification}/${a.qualState}; deliv=${a.delivery}/${a.materialRevision}; comp=${a.competitive}`;
  const gap = `${a.action}: impl ${a.implState}, verification ${a.verification}, qualification ${a.qualification}, delivery ${a.delivery} → RELEASED`;
  const acceptance = (a.acceptanceBoundary === "—" ? "RELEASED boundary per canon" : a.acceptanceBoundary).replace(/\|/g, "\\|");
  md.push(`| ${a.id} | ${a.lane} (${a.milestone}) | ${prod} | ${proof} | ${acceptance} | ${a.dependencies.replace(/\|/g, "\\|")} | ${gap} | ${a.evidence.replace(/\|/g, "\\|")} |`);
}
md.push("");

writeFileSync(join(HERE, "atoms.json"), `${JSON.stringify(atoms, null, 2)}\n`);
writeFileSync(join(HERE, "matrix.md"), md.join("\n"));
console.log(`atoms=${atoms.length} lanes=${JSON.stringify(laneCounts)}`);
