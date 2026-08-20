#!/usr/bin/env node
// CX-B7 behavioral report writer.
//
// Aggregates per-scenario trial results into pass^1/pass^3/pass^5 per the AX
// audit standard section 7.2, writes .audit/cx-b7/ax-report.json and a
// Markdown summary with an explicit UNPROVEN banner, and records the exact
// retirement condition under which the banner may be removed.
//
// Skipped-pending trials are reported separately from passes and are never
// folded into pass^k numerators or denominators.

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPORT_DIR_REL = [".audit", "cx-b7"];

export const UNPROVEN_BANNER = [
  "## UNPROVEN",
  "",
  "Blueprint's end-to-end behavioral AX is formally UNPROVEN. The stub driver is",
  "harness validation only: a stub pass proves the harness runs the scenarios",
  "and enforces final-environment-before-claims, it does NOT prove that a real",
  "agent achieves the scenario goals. No real-agent (claude) matrix has been",
  "executed, so no claim about real-agent AX reliability may be made from this",
  "report.",
].join("\n");

export const RETIREMENT_CONDITION = [
  "Retirement condition (exact): the UNPROVEN banner is removed only when ALL",
  "of the following hold in a single committed report:",
  "  1. every scenario in evals/ax/scenarios (currently 12) has been executed",
  "     by at least one real agent through --driver claude (never in CI);",
  "  2. the executed matrix covers at least two distinct real agents/models;",
  "  3. each matrix cell ran at least 5 independent trials per scenario and",
  "     produced a verified final environment before any claim was admitted;",
  "  4. pass^1/pass^3/pass^5 in that report meet the release thresholds the",
  "     owner sets at the time, with skipped-pending reported separately;",
  "  5. claim-fidelity checks (forbidden claims, forbidden reason codes,",
  "     forbidden operations) passed for every executed cell, and",
  "  6. the report records the exact commit, environment, driver version,",
  "     and agent/model versions that produced it.",
  "Until then the banner stays; removing it without that evidence is an",
  "unsupported claim and fails the claim-fidelity gate.",
].join("\n");

function percent(value) {
  if (value === null || value === undefined) return "n/a";
  return `${(value * 100).toFixed(1)}%`;
}

function markdownEscape(text) {
  return String(text ?? "").replace(/\|/g, "\\|").replace(/\r?\n/g, " ");
}

export function renderMarkdown(report) {
  const lines = [];
  lines.push("# CX-B7 behavioral AX report");
  lines.push("");
  lines.push(`- Suite: ${report.suite} (runner ${report.runner}, reporter ${report.reporter})`);
  lines.push(`- Generated: ${report.generatedAt}`);
  lines.push(`- Driver: \`${report.driver}\``);
  lines.push(`- Trials per scenario (--k): ${report.k}`);
  lines.push(`- Scenarios: ${report.scenarioCount}${report.scenarioCount !== report.expectedScenarioCount ? ` (expected ${report.expectedScenarioCount})` : ""}`);
  lines.push(`- Root: \`${report.root}\``);
  lines.push(`- Scenario dir: \`${report.scenarioDir}\``);
  lines.push(`- Environment: gitHead=${report.environment?.gitHead ?? "n/a"}, node=${report.environment?.nodeVersion ?? "n/a"}, platform=${report.environment?.platform ?? "n/a"}, CI=${report.environment?.env?.CI ?? "unset"}, network=${report.environment?.network ?? "n/a"}`);
  lines.push(`- Verdict: **${String(report.verdict ?? "fail").toUpperCase()}**`);
  lines.push("");
  lines.push(UNPROVEN_BANNER);
  lines.push("");
  lines.push("## pass^k (AX audit standard §7.2)");
  lines.push("");
  lines.push("| measure | value |");
  lines.push("|---|---|");
  for (const key of ["pass^1", "pass^3", "pass^5"]) {
    const entry = report.passK?.[key];
    if (!entry) {
      lines.push(`| ${key} | missing |`);
      continue;
    }
    if (entry.value === null) {
      lines.push(`| ${key} | ${entry.status ?? "unproven"} — ${markdownEscape(entry.reason ?? "not computed")} |`);
    } else {
      lines.push(`| ${key} | ${percent(entry.value)} (${entry.numerator}/${entry.denominator}) |`);
    }
  }
  lines.push("");
  lines.push("## Per-scenario status");
  lines.push("");
  lines.push("| scenario | status | pass | fail | skipped-pending |");
  lines.push("|---|---|---|---|---|");
  for (const scenarioId of Object.keys(report.scenarioResults ?? {}).sort()) {
    const result = report.scenarioResults[scenarioId];
    lines.push(`| ${scenarioId} | ${result.status} | ${result.passCount} | ${result.failCount} | ${result.skippedCount} |`);
  }
  lines.push("");
  lines.push("Skipped-pending is separate from passes: skipped-pending trials are");
  lines.push("never counted as passes and never enter pass^k numerators or denominators.");
  lines.push("");
  if ((report.skippedPending ?? []).length) {
    lines.push("### Skipped-pending");
    lines.push("");
    for (const entry of report.skippedPending) {
      lines.push(`- ${entry.scenarioId} (${entry.count}): ${markdownEscape((entry.reasons ?? []).join("; ") || "no reason")}`);
    }
    lines.push("");
  }
  lines.push("## Totals");
  lines.push("");
  lines.push(`- pass: ${report.totals?.pass ?? 0}`);
  lines.push(`- fail: ${report.totals?.fail ?? 0}`);
  lines.push(`- skipped-pending: ${report.totals?.["skipped-pending"] ?? 0}`);
  lines.push(`- trials run: ${report.totals?.trialsRun ?? 0}`);
  lines.push("");
  lines.push("## Checks");
  lines.push("");
  lines.push("| check | value |");
  lines.push("|---|---|");
  for (const [name, value] of Object.entries(report.checks ?? {})) {
    lines.push(`| ${name} | ${value === true ? "pass" : value === false ? "fail" : markdownEscape(String(value))} |`);
  }
  lines.push("");
  if (report.fatal) {
    lines.push("## Fatal");
    lines.push("");
    lines.push(`- ${markdownEscape(report.fatal.code)}: ${markdownEscape(report.fatal.message)}`);
    lines.push("");
  }
  lines.push("## Retirement condition");
  lines.push("");
  lines.push(RETIREMENT_CONDITION);
  lines.push("");
  lines.push("## Trial detail");
  lines.push("");
  for (const entry of report.trials ?? []) {
    lines.push(`### ${entry.scenarioId} (version ${entry.version})`);
    lines.push("");
    for (const trial of entry.trials) {
      const env = trial.envVerified ? "env verified before claims" : "env NOT verified";
      const claims = trial.claimsChecked ? "claims checked" : "claims not checked";
      lines.push(`- trial ${trial.trial}: **${trial.status}** — ${env}, ${claims}${trial.failures?.length ? `; failures: ${markdownEscape(trial.failures.join(" | "))}` : ""}`);
    }
    lines.push("");
  }
  return lines.join("\n");
}

export function writeReportFiles(report, root) {
  const outDir = join(resolve(root), ...REPORT_DIR_REL);
  mkdirSync(outDir, { recursive: true });
  const reportJsonPath = join(outDir, "ax-report.json");
  const reportMarkdownPath = join(outDir, "ax-report.md");
  writeFileSync(reportJsonPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  writeFileSync(reportMarkdownPath, renderMarkdown(report), "utf8");
  return { reportJsonPath, reportMarkdownPath };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  console.error("report.mjs is a library used by evals/ax/run-behavioral.mjs; run that instead.");
  process.exitCode = 2;
}
