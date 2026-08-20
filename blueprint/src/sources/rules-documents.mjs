// Adapter 5 — Applicable rules and project-level instructions.
//
// Collects the always-applicable rule corpus for a task:
//   - <repoRoot>/AGENTS.md (and nested AGENTS.md up to depth 4)
//   - <repoRoot>/.claude/rules/*.md (always-loaded compact index, plus
//     rule files surfaced by their filename matching the task terms)
//   - <repoRoot>/.codex/rules/*.md (Codex-translated surface, optional)
//   - <repoRoot>/CLAUDE.md when present
//
// Scope-aware selection:
//   - AGENTS.md and CLAUDE.md are ALWAYS admitted (root instruction layer).
//   - Rule files are admitted only when their filename or front matter tags
//     intersect with task terms OR when the user explicitly widened scope
//     with `scope.rulesInclude`.
//   - The dispatch requires: "rules must be scope-aware and must NOT flood
//     the packet merely because they are long." Per-rule byte cap defaults
//     to 1500 (plan-locked); long rules are truncated with a marker.
//
// All rule candidates carry `instructionPolicy="data_only"` and
// `trustClass="workspace_tracked"` so the planner treats them as source
// evidence rather than executable instructions — they inform the planner
// about constraints, they do not directly steer the model.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import {
  DEFAULT_RULE_BYTE_CAP,
  SCOPE_PROVIDER,
  isSupportedPath,
  makeCandidate,
  normalizePath,
  readFileBounded,
  safeResolve,
} from "./_shared.mjs";

const ADAPTER_ID = "membrane-sources/rules-documents";
const ADAPTER_LAYER = 2; // Layer 2 — instruction / rules corpus
const MAX_NESTED_AGENTS_DEPTH = 4;

const RULE_FILENAME_STOPWORDS = new Set([
  "the", "a", "an", "and", "or", "for", "to", "in", "of", "on", "with",
]);

function tokenise(s) {
  return String(s ?? "")
    .toLowerCase()
    .split(/[^a-z0-9_-]+/)
    .filter((t) => t.length >= 3 && !RULE_FILENAME_STOPWORDS.has(t));
}

function termsOverlap(taskTerms, ruleTerms) {
  for (const a of taskTerms) for (const b of ruleTerms) if (a === b) return true;
  return false;
}

function walkForAgentsMd(repoRoot, depth, maxDepth, out) {
  if (depth > maxDepth) return;
  let entries;
  try {
    entries = readdirSync(repoRoot, { withFileTypes: true });
  } catch {
    return;
  }
  const agentsPath = join(repoRoot, "AGENTS.md");
  if (existsSync(agentsPath)) {
    try {
      if (statSync(agentsPath).isFile()) out.push(agentsPath);
    } catch { /* unreadable; skip */ }
  }
  const claudePath = join(repoRoot, "CLAUDE.md");
  if (existsSync(claudePath)) {
    try {
      if (statSync(claudePath).isFile()) out.push(claudePath);
    } catch { /* unreadable; skip */ }
  }
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    if (entry.name === "node_modules" || entry.name === ".git" || entry.name === "dist" || entry.name === "build") continue;
    walkForAgentsMd(join(repoRoot, entry.name), depth + 1, maxDepth, out);
  }
}

function ruleFilesIn(repoRoot, relDir) {
  const out = [];
  const abs = join(repoRoot, relDir);
  if (!existsSync(abs)) return out;
  let entries;
  try {
    entries = readdirSync(abs, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    if (!entry.name.endsWith(".md")) continue;
    out.push(join(abs, entry.name));
  }
  return out;
}

function frontMatterTags(absolutePath) {
  // Tolerant YAML-ish front-matter scan: looks for `tags: [a, b]` or
  // `tag: a` on the first 20 lines. Real front matter is not parsed; this
  // is good enough for filenames that don't intersect task terms.
  let raw;
  try {
    raw = readFileSync(absolutePath, "utf8");
  } catch {
    return [];
  }
  const lines = raw.split(/\r?\n/).slice(0, 20);
  const tags = [];
  let inTags = false;
  for (const line of lines) {
    if (/^---\s*$/.test(line)) {
      if (inTags) break;
      inTags = true;
      continue;
    }
    if (!inTags) continue;
    const tagMatch = line.match(/^\s*tags?\s*:\s*(.*)$/i);
    if (tagMatch) {
      const body = tagMatch[1].trim();
      if (body.startsWith("[") && body.endsWith("]")) {
        for (const tok of body.slice(1, -1).split(",")) tags.push(tok.trim().toLowerCase());
      } else if (body) {
        tags.push(body.toLowerCase());
      }
    }
  }
  return tags.filter(Boolean);
}

function isAdmissibleRuleFile(absolutePath, taskTerms, scope) {
  const baseName = absolutePath.split(/[\\/]/).pop() ?? "";
  const stemTokens = tokenise(baseName.replace(/\.md$/, ""));
  const tags = frontMatterTags(absolutePath);
  if (scope?.rulesInclude && Array.isArray(scope.rulesInclude) && scope.rulesInclude.length) {
    return scope.rulesInclude.some((needle) => baseName.includes(needle) || stemTokens.includes(needle.toLowerCase()));
  }
  if (scope?.rulesExclude && Array.isArray(scope.rulesExclude) && scope.rulesExclude.some((needle) => baseName.includes(needle))) {
    return false;
  }
  if (tags.length && termsOverlap(taskTerms, tags)) return true;
  if (stemTokens.length === 0) return true; // a rule with no name still gets included with a low priority
  return termsOverlap(taskTerms, stemTokens);
}

function scoreForRule(meta) {
  let score = 0.55;
  if (meta.isRoot) score += 0.3;
  if (meta.matched) score += 0.1;
  if (meta.tagged) score += 0.05;
  return Math.min(1, score);
}

/**
 * @param {string} task
 * @param {{ repoRoot: string, ruleByteCap?: number, rulesInclude?: string[], rulesExclude?: string[], maxRules?: number }} scope
 */
export function produce(task, scope) {
  const repoRoot = scope?.repoRoot;
  if (!repoRoot) return { candidates: [], omissions: [] };
  const taskTerms = tokenise(task);
  const byteCap = Number(scope?.ruleByteCap) > 0 ? Number(scope.ruleByteCap) : DEFAULT_RULE_BYTE_CAP;
  const maxRules = Number(scope?.maxRules) > 0 ? Math.floor(Number(scope.maxRules)) : 24;

  const candidates = [];
  const omissions = [];

  const agentsFiles = [];
  walkForAgentsMd(repoRoot, 0, MAX_NESTED_AGENTS_DEPTH, agentsFiles);
  for (const absolute of agentsFiles) {
    const safe = safeResolve(repoRoot, absolute);
    if (!safe) {
      omissions.push({ id: `${ADAPTER_ID}:${normalizePath(absolute)}`, layer: ADAPTER_LAYER, reason: "rule_outside_scope" });
      continue;
    }
    const body = readFileBounded(absolute, byteCap);
    if (!body) continue;
    const isRoot = safe === "AGENTS.md" || safe === "CLAUDE.md";
    candidates.push(
      makeCandidate({
        id: `${ADAPTER_ID}:${safe}`,
        layer: ADAPTER_LAYER,
        sourceKind: "instruction",
        sourcePath: safe,
        trustClass: "workspace_tracked",
        providerScore: scoreForRule({ isRoot, matched: false, tagged: false }),
        scoreComponents: { rule: 1.0, root: isRoot ? 1.0 : 0.5 },
        text: body.text,
        startLine: 1,
        endLine: body.lines,
        bodyHash: body.bodyHash,
        estimatedTokens: Math.max(1, body.lines),
        resolver: `Read ${safe}`,
      }),
    );
  }

  const ruleDirs = [".claude/rules", ".codex/rules"];
  const ruleSet = [];
  for (const dir of ruleDirs) {
    for (const absolute of ruleFilesIn(repoRoot, dir)) {
      const safe = safeResolve(repoRoot, absolute);
      if (!safe) continue;
      if (!isSupportedPath(safe)) continue;
      if (!isAdmissibleRuleFile(absolute, taskTerms, scope)) continue;
      const tags = frontMatterTags(absolute);
      ruleSet.push({ absolute, safe, tags });
    }
  }

  // Sort: most-named overlap with task first, then root-name matches, then alphabetic.
  ruleSet.sort((a, b) => {
    const aHits = tokenise(a.safe.replace(/\.md$/, "").split(/[\\/]/).pop() ?? "").filter((t) => taskTerms.includes(t)).length;
    const bHits = tokenise(b.safe.replace(/\.md$/, "").split(/[\\/]/).pop() ?? "").filter((t) => taskTerms.includes(t)).length;
    if (bHits !== aHits) return bHits - aHits;
    return a.safe.localeCompare(b.safe);
  });

  for (const rule of ruleSet.slice(0, maxRules)) {
    const body = readFileBounded(rule.absolute, byteCap);
    if (!body) continue;
    candidates.push(
      makeCandidate({
        id: `${ADAPTER_ID}:${rule.safe}`,
        layer: ADAPTER_LAYER,
        sourceKind: "instruction",
        sourcePath: rule.safe,
        trustClass: "workspace_tracked",
        providerScore: scoreForRule({
          isRoot: false,
          matched: true,
          tagged: rule.tags.length > 0,
        }),
        scoreComponents: { rule: 0.8, lexical: 0.6 },
        text: body.text,
        startLine: 1,
        endLine: body.lines,
        bodyHash: body.bodyHash,
        estimatedTokens: Math.max(1, body.lines),
        resolver: `Read ${rule.safe}`,
      }),
    );
  }

  // If user gave NO task terms and asked for "always-on" rules, surface every
  // file with low priority. The cap prevents flooding.
  if (!taskTerms.length && !scope?.rulesInclude) {
    // already covered above — root files only
  }

  return { candidates, omissions };
}

export const adapterInfo = {
  id: ADAPTER_ID,
  layer: ADAPTER_LAYER,
  provider: SCOPE_PROVIDER,
  description: "Scope-aware AGENTS.md / .claude/rules / .codex/rules with per-rule byte cap (default 1500).",
};

export const _internals = {
  isAdmissibleRuleFile,
  scoreForRule,
  tokenise,
  walkForAgentsMd,
  ruleFilesIn,
};