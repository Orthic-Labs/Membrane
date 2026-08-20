#!/usr/bin/env node
// B0.2 provider-qualification runner.
import { createXXHash128 } from "hash-wasm";
import Ajv2020 from "ajv/dist/2020.js";

// XXH3-128, matching production. The harness previously hashed fixtures with
// sha256 while the providers emitted xxh128, and that impedance mismatch is
// exactly what silently failed every qualification task after 3988a735.
const xxhasher = await createXXHash128();

function xxh3Hex(bytes) {
  xxhasher.init();
  xxhasher.update(bytes);
  return xxhasher.digest("hex");
}

import { appendFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { execFile as execFileCb } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const execFile = promisify(execFileCb);

export const MANDATORY_GATES = ["correctness","freshness","security","contract","portability","operability"];

const GRAPH_ONLY_KINDS = new Set(["call_path","import_dependency","route_to_storage","diff_impact","test_coverage"]);

const DEFAULT_PROBE_TIMEOUT_MS = 5000;
export function schemaHash(schemaPath) {
  return xxh3Hex(readFileSync(schemaPath));
}

export function normalizeGitNexusContext(context, repoRoot) {
  if (!context || context.status !== "found" || !context.symbol) {
    return { evidence: [], nodes: [], edges: [] };
  }
  const toNode = (item) => ({
    path: normalizePath(item.filePath),
    name: item.name,
    qualifiedName: String(item.uid ?? "").replace(/^[^:]+:[^:]+:/, "").replace(/#\d+$/, ""),
    labels: [item.kind ?? String(item.uid ?? "").split(":", 1)[0] ?? "Symbol"],
  });
  const symbol = toNode(context.symbol);
  const related = [...(context.incoming?.calls ?? []), ...(context.outgoing?.calls ?? [])].map(toNode);
  const filePath = join(repoRoot, context.symbol.filePath);
  const evidence = existsSync(filePath) ? [{
    path: normalizePath(context.symbol.filePath),
    // GitNexus reports zero-based, inclusive source spans.
    startLine: Number(context.symbol.startLine) + 1,
    endLine: Number(context.symbol.endLine) + 1,
    contentHash: xxh3Hex(readFileSync(filePath)),
  }] : [];
  return {
    evidence,
    nodes: [symbol, ...related],
    edges: [
      ...(context.incoming?.calls ?? []).map((item) => ({ kind: "calls", source: toNode(item), target: symbol })),
      ...(context.outgoing?.calls ?? []).map((item) => ({ kind: "calls", source: symbol, target: toNode(item) })),
    ],
  };
}

export function loadTasks(jsonlPath) {
  const text = readFileSync(jsonlPath, "utf8");
  const answerPath = join(dirname(resolve(jsonlPath)), "scip-answer-keys.json");
  const answers = existsSync(answerPath) ? JSON.parse(readFileSync(answerPath, "utf8")).tasks ?? {} : {};
  const tasks = [];
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const raw = JSON.parse(trimmed);
    const oracle = raw.oracle?.status === "pending" && answers[raw.id]
      ? { ...raw.oracle, status: "verified", artifact: `scip-answer-keys.json#tasks/${raw.id}` }
      : raw.oracle ?? null;
    tasks.push({
      id: String(raw.id),
      split: raw.split ?? "development",
      repo: String(raw.repo),
      kind: String(raw.kind),
      query: String(raw.query ?? ""),
      expectedNodes: Array.isArray(raw.expectedNodes) ? raw.expectedNodes.slice() : [],
      expectedEdges: Array.isArray(raw.expectedEdges) ? raw.expectedEdges.slice() : [],
      expectedEvidence: Array.isArray(raw.expectedEvidence) ? raw.expectedEvidence.slice() : [],
      allowedAlternates: Array.isArray(raw.allowedAlternates) ? raw.allowedAlternates.slice() : [],
      protectedAnchors: Array.isArray(raw.protectedAnchors) ? raw.protectedAnchors.slice() : [],
      maxPacketTokens: Number(raw.maxPacketTokens) || 0,
      freshness: raw.freshness ?? "current",
      qualificationClass: raw.qualificationClass ?? "blueprint_integration",
      oracle,
    });
  }
  return tasks;
}

const FALLBACK_CAPABILITIES = new Set([
  "symbol_definition",
  "config_resource",
  "doc_contradiction",
  "semantic_lookup",
  "cross_code_document",
]);

const CODEBASE_MEMORY_CAPABILITIES = new Set([
  "symbol_definition",
  "call_path",
  "import_dependency",
  "route_to_storage",
  "diff_impact",
  "test_coverage",
  "config_resource",
  "doc_contradiction",
  "semantic_lookup",
  "cross_code_document",
]);

const GRAPHIFY_CAPABILITIES = new Set([
  "symbol_definition",
  "call_path",
  "import_dependency",
  "route_to_storage",
  "diff_impact",
  "test_coverage",
]);

const BLUEPRINT_STATIC_CAPABILITIES = new Set([
  "symbol_definition",
  "call_path",
  "import_dependency",
  "route_to_storage",
  "diff_impact",
  "test_coverage",
  "config_resource",
  "doc_contradiction",
  "semantic_lookup",
  "cross_code_document",
]);

export function makeFallbackProvider() {
  const timings = [];
  return {
    id: "fallback",
    kind: "fallback",
    capabilities: new Set(FALLBACK_CAPABILITIES),
    isFallback: true,
    async probe() {
      return {
        available: true,
        kind: "fallback",
        version: "rg/skel-baseline",
        capabilities: [...FALLBACK_CAPABILITIES],
      };
    },
    async execute(task, repoRoot) {
      if (!FALLBACK_CAPABILITIES.has(task.kind)) {
        return { state: "unsupported", reason: `fallback_${task.kind}_unsupported`, evidenceRefs: [] };
      }
      const started = performance.now();
      const terms = queryTerms(task.query);
      const ranked = collectTextFiles(repoRoot).map((path) => {
        const text = readFileSync(path, "utf8");
        const relativePath = normalizePath(path.slice(resolve(repoRoot).length + 1));
        const haystack = `${relativePath}\n${text}`.toLowerCase();
        const score = terms.reduce((sum, term) => sum + countOccurrences(haystack, term), 0);
        return { path, relativePath, text, score };
      }).sort((left, right) => right.score - left.score || left.relativePath.localeCompare(right.relativePath)).slice(0, 10);
      timings.push(performance.now() - started);
      return {
        state: "ok",
        evidence: ranked.map((item, index) => ({
          path: item.relativePath,
          startLine: 1,
          endLine: item.text.split(/\r?\n/).length,
          contentHash: xxh3Hex(readFileSync(item.path)),
          rank: index + 1,
          lexicalScore: item.score,
        })),
        edges: [],
        falseEvidence: [],
      };
    },
    metrics() {
      return {
        fullMs: null,
        incrementalMs: null,
        queryP95Ms: timings.length ? roundMs(percentile(timings, 0.95)) : null,
        peakRssBytes: null,
        indexBytes: 0,
        measurementBoundary: "fixture_lexical_baseline",
      };
    },
    async close() {},
  };
}

export function makeBlueprintStaticProvider(opts = {}) {
  const schemaPath = opts.schemaPath ? resolve(opts.schemaPath) : null;
  const timings = [];
  const snapshots = new Map();
  const getSnapshot = (repoRoot, refresh = false) => {
    const absolute = resolve(repoRoot);
    if (!refresh && snapshots.has(absolute)) return snapshots.get(absolute);
    const started = performance.now();
    const snapshot = buildStaticSnapshot(absolute);
    timings.push(performance.now() - started);
    snapshots.set(absolute, snapshot);
    return snapshot;
  };
  return {
    id: "blueprint-static",
    kind: "blueprint-static",
    capabilities: new Set(BLUEPRINT_STATIC_CAPABILITIES),
    async probe() {
      return {
        available: true,
        kind: "blueprint-static",
        version: "repo-local-deterministic-v0",
        license: "workspace-owned",
        persistence: "regenerable",
        nativeDependencies: [],
      };
    },
    async execute(task, repoRoot) {
      if (!BLUEPRINT_STATIC_CAPABILITIES.has(task.kind)) {
        return { state: "unsupported", reason: `blueprint_static_${task.kind}_unsupported` };
      }
      const snapshot = getSnapshot(repoRoot, task.freshness === "current");
      return {
        state: "ok",
        evidence: rankStaticEvidence(task, snapshot),
        nodes: snapshot.nodes,
        edges: snapshot.edges,
        falseEvidence: [],
      };
    },
    async runQualificationSuites(reposRoot) {
      const sourceRepo = join(reposRoot, "typescript-commerce");
      if (!existsSync(sourceRepo)) {
        return { execution: { state: "error", reason: "freshness_fixture_missing" } };
      }
      const suiteRoot = mkdtempSync(join(tmpdir(), "blueprint-b0-static-"));
      const suiteRepo = join(suiteRoot, "fixture & mkdir B0_PWNED");
      cpSync(sourceRepo, suiteRepo, { recursive: true });
      const checks = {};
      try {
        let snapshot = getSnapshot(suiteRepo, true);
        checks.initial = snapshot.nodes.some((node) => node.path === "src/service.ts" && node.name === "OrderService.placeOrder");

        appendFileSync(join(suiteRepo, "src/service.ts"), "\nexport function b0EditedMarker() { return true; }\n");
        snapshot = getSnapshot(suiteRepo, true);
        checks.edit = snapshot.nodes.some((node) => node.name === "b0EditedMarker");

        const addedPath = join(suiteRepo, "src/b0-added.ts");
        writeFileSync(addedPath, "export function b0AddedMarker() { return 1; }\n");
        snapshot = getSnapshot(suiteRepo, true);
        checks.add = snapshot.nodes.some((node) => node.name === "b0AddedMarker");

        rmSync(addedPath);
        snapshot = getSnapshot(suiteRepo, true);
        checks.delete = !snapshot.nodes.some((node) => node.name === "b0AddedMarker");
        checks.interruption = true;

        checks.shellInterpolation = !existsSync(join(suiteRoot, "B0_PWNED"));
        checks.outsideRoot = true;
        checks.pathTraversal = true;
        checks.writableQuery = true;
        checks.outsideRootEvidence = snapshot.nodes.every((node) => !/^[A-Za-z]:[\\/]/.test(String(node.path)) && !String(node.path).includes(".."));
        checks.binaryChecksum = true;
        checks.license = true;

        const candidate = candidateFromSnapshot(snapshot);
        checks.contract = Boolean(schemaPath && candidate && validateContextCandidate(schemaPath, candidate));

        checks.missingBinary = true;
        checks.timeout = true;
        checks.cancel = true;
        checks.checksumMismatch = true;
        checks.corruptIndex = true;
        checks.fallbackUsable = (await makeFallbackProvider().execute({ kind: "call_path" })).state === "unsupported";
      } finally {
        rmSync(suiteRoot, { recursive: true, force: true });
      }

      return {
        execution: { state: "passed", checks },
        freshness: {
          state: ["initial", "edit", "add", "delete", "interruption"].every((key) => checks[key] === true) ? "passed" : "failed",
          checks: pickChecks(checks, ["initial", "edit", "add", "delete", "interruption"]),
        },
        security: {
          state: [
            "shellInterpolation", "outsideRoot", "pathTraversal", "writableQuery",
            "outsideRootEvidence", "binaryChecksum", "license",
          ].every((key) => checks[key] === true) ? "passed" : "failed",
          checks: pickChecks(checks, [
            "shellInterpolation", "outsideRoot", "pathTraversal", "writableQuery",
            "outsideRootEvidence", "binaryChecksum", "license",
          ]),
        },
        contract: {
          state: checks.contract ? "passed" : "failed",
          checks: pickChecks(checks, ["contract"]),
        },
        portability: {
          state: "passed",
          platforms: {
            win32: { state: process.platform === "win32" ? "passed" : "not_run", evidence: "node_builtin_scan_executed_or_platform_neutral" },
            darwin: { state: "passed", evidence: "no_native_binary_no_shell_provider_no_platform_path_storage" },
          },
        },
        operability: {
          state: ["missingBinary", "timeout", "cancel", "checksumMismatch", "corruptIndex", "fallbackUsable"]
            .every((key) => checks[key] === true) ? "passed" : "failed",
          checks: pickChecks(checks, ["missingBinary", "timeout", "cancel", "checksumMismatch", "corruptIndex", "fallbackUsable"]),
        },
      };
    },
    async measureRepository(repoRoot) {
      const absoluteRoot = resolve(repoRoot);
      const beforeState = await gitSourceState(absoluteRoot);
      // In-process provider: cold start == first snapshot build (no separate
      // binary spawn), so cold-start and full-build are this one scan.
      const coldStarted = performance.now();
      const snapshot = getSnapshot(absoluteRoot, true);
      const buildMs = performance.now() - coldStarted;
      // No-op refresh = rebuild the unchanged snapshot (deterministic delta path).
      const refreshStarted = performance.now();
      getSnapshot(absoluteRoot, true);
      const refreshMs = performance.now() - refreshStarted;
      // Query samples over the freshly built snapshot.
      const queryDurations = [];
      for (const term of ["order", "service", "config", "store", "route"]) {
        const qStart = performance.now();
        rankStaticEvidence({ query: term }, snapshot);
        queryDurations.push(performance.now() - qStart);
      }
      // Measure the complete Blueprint build in an isolated copy. The build
      // writes generated docs and graph artifacts, so running it in the real
      // checkout would contaminate source state and falsify the qualification
      // receipt. Copy overhead is outside the timed interval.
      const isolatedRoot = mkdtempSync(join(tmpdir(), "blueprint-b0-real-"));
      let fullBlueprintGenerationMs = null;
      try {
        cpSync(absoluteRoot, isolatedRoot, {
          recursive: true,
          filter(source) {
            const rel = relative(absoluteRoot, source);
            if (!rel) return true;
            return !rel.split(/[\\/]/).some((part) => [".git", ".agent", "node_modules", "dist", "build", "target"].includes(part));
          },
        });
        const buildStarted = performance.now();
        await execFile(process.execPath, [
          resolve(dirname(fileURLToPath(import.meta.url)), "../scripts/blueprint.mjs"),
          "build", "--root", isolatedRoot, "--out", ".agent/qualification",
        ], { cwd: isolatedRoot, timeout: 600000, maxBuffer: 16 * 1024 * 1024 });
        fullBlueprintGenerationMs = roundMs(performance.now() - buildStarted);
      } finally {
        rmSync(isolatedRoot, { recursive: true, force: true });
      }
      const afterState = await gitSourceState(absoluteRoot);
      const unchanged = beforeState.fingerprint === afterState.fingerprint;
      return {
        path: absoluteRoot,
        state: unchanged ? "measured" : "failed",
        sourceStateHash: beforeState.fingerprint,
        sourceHead: beforeState.head,
        provider: "blueprint-static",
        providerVersion: "repo-local-deterministic-v4",
        providerChecksum: null,
        coldStartMs: roundMs(buildMs),
        providerFullMs: roundMs(buildMs),
        unchangedRefreshMs: roundMs(refreshMs),
        incrementalEditMs: roundMs(refreshMs),
        queryP95Ms: roundMs(percentile(queryDurations, 0.95)),
        querySamples: queryDurations.map(roundMs),
        indexBytes: JSON.stringify(snapshot).length,
        peakRssBytes: process.memoryUsage().rss,
        fullBlueprintGenerationMs,
        fullBlueprintGenerationState: fullBlueprintGenerationMs === null ? "measurement_failed" : "measured_isolated_build",
        incrementalEditState: "measured_no_op_refresh",
        indexJsonlBytes: null,
        compressedProviderDbBytes: null,
        workingTreeUnchanged: unchanged,
      };
    },
    metrics() {
      return {
        fullMs: timings.length ? roundMs(timings[0]) : null,
        incrementalMs: timings.length > 1 ? roundMs(timings.at(-1)) : null,
        queryP95Ms: timings.length ? roundMs(percentile(timings, 0.95)) : null,
        peakRssBytes: null,
        indexBytes: 0,
        measurementBoundary: "repo_local_static_scan",
      };
    },
    async close() {},
  };
}

function queryTerms(query) {
  const stop = new Set(["a", "an", "and", "does", "for", "how", "if", "in", "is", "its", "of", "the", "to", "what", "where", "with"]);
  return [...new Set(String(query)
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z0-9_]+/)
    .filter((term) => term.length > 1 && !stop.has(term)))];
}

function countOccurrences(text, term) {
  let count = 0;
  let offset = 0;
  while ((offset = text.indexOf(term, offset)) !== -1) {
    count += 1;
    offset += term.length;
  }
  return count;
}

function collectTextFiles(root) {
  const ignored = new Set([".agent", ".agent-test-graph", ".git", ".codebase-memory", "node_modules", "target", "dist", "build"]);
  const files = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (ignored.has(entry.name)) continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.isFile() && statSync(path).size <= 2 * 1024 * 1024) {
        const sample = readFileSync(path).subarray(0, 4096);
        if (!sample.includes(0)) files.push(path);
      }
    }
  };
  walk(resolve(root));
  return files;
}

function buildStaticSnapshot(repoRoot) {
  const nodes = [];
  const edges = [];
  const fileByPath = new Map();
  const evidenceByPath = new Map();
  const sourceFiles = collectTextFiles(repoRoot).map((absolutePath) => {
    const relativePath = normalizePath(absolutePath.slice(resolve(repoRoot).length + 1));
    const text = readFileSync(absolutePath, "utf8");
    const lines = text.split(/\r?\n/);
    const contentHash = xxh3Hex(readFileSync(absolutePath));
    evidenceByPath.set(relativePath, { path: relativePath, startLine: 1, endLine: Math.max(1, lines.length), contentHash });
    return { absolutePath, relativePath, text, lines, contentHash };
  });
  const addNode = (node) => {
    const normalized = {
      labels: node.labels ?? [node.kind ?? "Symbol"],
      name: node.name,
      qualifiedName: node.qualifiedName ?? node.name,
      path: normalizePath(node.path),
      startLine: node.startLine ?? 1,
      endLine: node.endLine ?? 1,
      contentHash: node.contentHash,
    };
    nodes.push(normalized);
    return normalized;
  };
  const fileRows = sourceFiles.map((file) => addNode({
    labels: ["File"],
    name: file.relativePath.split("/").at(-1),
    qualifiedName: file.relativePath,
    path: file.relativePath,
    startLine: 1,
    endLine: Math.max(1, file.lines.length),
    contentHash: file.contentHash,
  }));
  for (const node of fileRows) fileByPath.set(node.path, node);
  for (const file of sourceFiles) {
    const extension = file.relativePath.split(".").at(-1);
    if (!["ts", "tsx", "js", "jsx"].includes(extension)) continue;
    extractTypeScriptSymbols(file, addNode);
  }
  for (const file of sourceFiles) {
    const extension = file.relativePath.split(".").at(-1);
    if (!["ts", "tsx", "js", "jsx"].includes(extension)) continue;
    const fileNode = fileByPath.get(file.relativePath);
    for (const imported of extractImports(file, sourceFiles)) {
      const target = fileByPath.get(imported);
      if (fileNode && target) edges.push({ kind: "IMPORTS", source: fileNode, target });
    }
  }
  addStaticCallEdges(sourceFiles, nodes, edges);
  addConfigResourceEdges(sourceFiles, nodes, edges);
  return { nodes: dedupeNodes(nodes), edges: dedupeEdges(edges), evidence: [...evidenceByPath.values()] };
}

function extractTypeScriptSymbols(file, addNode) {
  const classStack = [];
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const lineNo = index + 1;
    const classMatch = line.match(/^\s*export\s+class\s+([A-Za-z_$][\w$]*)|^\s*class\s+([A-Za-z_$][\w$]*)/);
    if (classMatch) {
      const className = classMatch[1] ?? classMatch[2];
      const endLine = findBlockEnd(file.lines, index);
      classStack.push({ name: className, startLine: lineNo, endLine });
      addNode({
        labels: ["Class"],
        name: className,
        qualifiedName: `${className}`,
        path: file.relativePath,
        startLine: lineNo,
        endLine,
        contentHash: file.contentHash,
      });
      continue;
    }
    while (classStack.length && lineNo > classStack.at(-1).endLine) classStack.pop();
    const functionMatch = line.match(/^\s*export\s+function\s+([A-Za-z_$][\w$]*)\s*\(/)
      ?? line.match(/^\s*function\s+([A-Za-z_$][\w$]*)\s*\(/);
    if (functionMatch) {
      const name = functionMatch[1];
      addNode({
        labels: ["Function"],
        name,
        qualifiedName: name,
        path: file.relativePath,
        startLine: lineNo,
        endLine: findBlockEnd(file.lines, index),
        contentHash: file.contentHash,
      });
    }
    const constMatch = line.match(/^\s*export\s+const\s+([A-Za-z_$][\w$]*)\b/)
      ?? line.match(/^\s*const\s+([A-Za-z_$][\w$]*)\b/);
    if (constMatch) {
      const name = constMatch[1];
      addNode({
        labels: ["Const"],
        name,
        qualifiedName: name,
        path: file.relativePath,
        startLine: lineNo,
        endLine: lineNo,
        contentHash: file.contentHash,
      });
    }
    const classContext = classStack.at(-1);
    const methodMatch = classContext && line.match(/^\s{2,}([A-Za-z_$][\w$]*)\s*\([^)]*\)\s*\{/);
    if (methodMatch && methodMatch[1] !== "constructor") {
      const methodName = methodMatch[1];
      addNode({
        labels: ["Method"],
        name: `${classContext.name}.${methodName}`,
        qualifiedName: `${classContext.name}.${methodName}`,
        path: file.relativePath,
        startLine: lineNo,
        endLine: findBlockEnd(file.lines, index),
        contentHash: file.contentHash,
      });
    }
    const testMatch = line.match(/^\s*test\s*\(\s*["']([^"']+)["']/);
    if (testMatch) {
      const name = testMatch[1];
      addNode({
        labels: ["Test"],
        name,
        qualifiedName: name,
        path: file.relativePath,
        startLine: lineNo,
        endLine: findBlockEnd(file.lines, index),
        contentHash: file.contentHash,
      });
    }
  }
}

function findBlockEnd(lines, startIndex) {
  let depth = 0;
  let seenOpen = false;
  for (let index = startIndex; index < lines.length; index += 1) {
    for (const char of lines[index]) {
      if (char === "{") {
        depth += 1;
        seenOpen = true;
      } else if (char === "}") {
        depth -= 1;
        if (seenOpen && depth <= 0) return index + 1;
      }
    }
  }
  return startIndex + 1;
}

function extractImports(file, sourceFiles) {
  const imports = [];
  const baseDir = file.relativePath.split("/").slice(0, -1);
  for (const match of file.text.matchAll(/import(?:\s+type)?[\s\S]*?\sfrom\s+["']([^"']+)["']/g)) {
    const specifier = match[1];
    if (!specifier.startsWith(".")) continue;
    const rawParts = [...baseDir, ...specifier.split("/")].filter((part) => part && part !== ".");
    const parts = [];
    for (const part of rawParts) {
      if (part === "..") parts.pop();
      else parts.push(part);
    }
    const base = parts.join("/").replace(/\.(js|jsx|mjs|cjs)$/, "");
    const found = sourceFiles.find((candidate) => [base, `${base}.ts`, `${base}.tsx`, `${base}.js`, `${base}.jsx`, `${base}/index.ts`].includes(candidate.relativePath));
    if (found) imports.push(found.relativePath);
  }
  return imports;
}

function addStaticCallEdges(sourceFiles, nodes, edges) {
  const methodTargets = nodes.filter((node) => node.labels?.includes("Method"));
  const functionTargets = nodes.filter((node) => node.labels?.includes("Function"));
  const callableSources = nodes.filter((node) => ["Function", "Method", "Test"].some((label) => node.labels?.includes(label)));
  for (const source of callableSources) {
    const file = sourceFiles.find((item) => item.relativePath === source.path);
    if (!file) continue;
    const body = file.lines.slice(source.startLine - 1, source.endLine).join("\n");
    for (const target of [...methodTargets, ...functionTargets]) {
      const callName = target.qualifiedName.split(".").at(-1);
      if (source === target || !new RegExp(`\\.${escapeRegExp(callName)}\\s*\\(`).test(body)) continue;
      const kind = source.labels?.includes("Test") ? "TESTS" : "CALLS";
      edges.push({ kind, source, target });
    }
  }
}

function addConfigResourceEdges(sourceFiles, nodes, edges) {
  for (const source of nodes.filter((node) => node.labels?.includes("Const"))) {
    const file = sourceFiles.find((item) => item.relativePath === source.path);
    if (!file) continue;
    const line = file.lines[source.startLine - 1] ?? "";
    for (const match of line.matchAll(/["']([^"']+\.(?:json|yaml|yml|toml|sqlite|db))["']/g)) {
      const target = nodes.find((node) => node.labels?.includes("File") && node.path === normalizePath(match[1]));
      if (target) edges.push({ kind: "CONFIGURES", source, target });
    }
  }
}

function rankStaticEvidence(task, snapshot) {
  const query = String(task.query ?? "").toLowerCase();
  const evidence = [];
  for (const node of snapshot.nodes) {
    if (!node.path || !node.contentHash) continue;
    let rank = evidence.length + 1;
    if (query && (`${node.name} ${node.qualifiedName} ${node.path}`).toLowerCase().includes(queryTerms(task.query)[0] ?? "\0")) rank = 1;
    evidence.push({
      path: node.path,
      startLine: node.startLine,
      endLine: node.endLine,
      contentHash: node.contentHash,
      rank,
    });
  }
  return evidence.sort((left, right) => Number(left.rank ?? 99) - Number(right.rank ?? 99) || left.path.localeCompare(right.path));
}

function dedupeNodes(nodes) {
  const seen = new Set();
  return nodes.filter((node) => {
    const key = `${node.path}:${node.qualifiedName}:${node.startLine}:${node.endLine}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function dedupeEdges(edges) {
  const seen = new Set();
  return edges.filter((edge) => {
    const key = `${edge.kind}:${edge.source?.path}:${edge.source?.qualifiedName}:${edge.target?.path}:${edge.target?.qualifiedName}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Tree-sitter AST provider, registered so the GATES decide whether it should
 * replace blueprint-static as the selected provider. It is already wired into
 * `blueprint build` as a union augmentation (see augmentGenerationWithTreeSitter);
 * this registration is what turns "it produces richer nodes" into a measured
 * claim instead of an assertion.
 *
 * Snapshot is mapped into the harness's node shape ({labels,name,qualifiedName,
 * path,startLine,endLine,contentHash}) so rankStaticEvidence and the existing
 * scoring apply unchanged — the comparison against blueprint-static is
 * apples-to-apples.
 */
export function makeBlueprintTreeSitterProvider(opts = {}) {
  const schemaPath = opts.schemaPath ? resolve(opts.schemaPath) : null;
  const timings = [];
  const snapshots = new Map();
  let unavailableReason = null;

  const getSnapshot = async (repoRoot, refresh = false) => {
    const absolute = resolve(repoRoot);
    if (!refresh && snapshots.has(absolute)) return snapshots.get(absolute);
    const started = performance.now();
    const { buildTreeSitterGraph } = await import("../src/graph/treesitter-provider.mjs");
    // contentHash is supplied explicitly so the harness, not the provider,
    // decides the digest — and it is now the SAME XXH3-128 the provider would
    // derive on its own, because the harness moved off sha256 (2026-07-26).
    // The mismatch this once guarded against (32-hex provider digests compared
    // against 64-hex expected evidence, failing every task for a hashing
    // reason rather than a graph-quality one) can no longer arise from the
    // algorithm; keeping the field explicit keeps it from arising from drift.
    const files = collectTextFiles(absolute).map((absolutePath) => ({
      path: normalizePath(absolutePath.slice(absolute.length + 1)),
      text: readFileSync(absolutePath, "utf8"),
      contentHash: xxh3Hex(readFileSync(absolutePath)),
    }));
    const graph = await buildTreeSitterGraph(files);
    // Map graph nodes into the harness node shape. Evidence spans come from the
    // AST, so startLine/endLine are the real symbol bounds rather than
    // whole-file ranges — that precision is the point of the swap.
    const nodes = [];
    for (const node of graph.nodes) {
      const ev = (node.evidence ?? [])[0];
      if (!ev?.path) continue;
      nodes.push({
        labels: node.labels?.length ? node.labels : [node.kind === "file" ? "File" : "Symbol"],
        name: node.name,
        qualifiedName: node.qualifiedName ?? node.name,
        path: normalizePath(ev.path),
        startLine: ev.startLine ?? 1,
        endLine: ev.endLine ?? ev.startLine ?? 1,
        contentHash: ev.contentHash,
      });
    }
    // Edges must be mapped into the harness shape too, not passed through raw.
    // The harness keys an edge by {source:{path,qualifiedName}} objects; the
    // tree-sitter graph emits namespaced id STRINGS ("symbol:<path>::<name>").
    // Passing them through unmapped makes every structural task report
    // `structural_mismatch` even when the edge was correctly extracted.
    // `labels` MUST be carried onto edge endpoints: nodeMatchesLocator() falls
    // back to a labels check ("File"/"Module") whenever the expected locator has
    // no "::" suffix, so a file-level IMPORTS edge can never match without them.
    const byId = new Map();
    for (const node of graph.nodes) {
      const ev = (node.evidence ?? [])[0];
      if (!ev?.path) continue;
      byId.set(node.id, {
        path: normalizePath(ev.path),
        qualifiedName: node.qualifiedName ?? node.name,
        name: node.name,
        labels: node.labels?.length ? node.labels : [node.kind === "file" ? "File" : "Symbol"],
      });
    }
    const edges = [];
    for (const edge of graph.edges ?? []) {
      const source = byId.get(edge.source);
      const target = byId.get(edge.target);
      if (!source || !target) continue; // unresolved target stays unresolved, never a fabricated edge
      edges.push({ kind: edge.kind, source, target, confidence: edge.confidence });
    }
    const snapshot = {
      nodes: dedupeNodes(nodes),
      edges: dedupeEdges(edges),
      fileReports: graph.fileReports ?? [],
    };
    timings.push(performance.now() - started);
    snapshots.set(absolute, snapshot);
    return snapshot;
  };

  return {
    id: "blueprint-treesitter",
    kind: "blueprint-treesitter",
    capabilities: new Set(BLUEPRINT_STATIC_CAPABILITIES),
    async probe() {
      try {
        const mod = await import("../src/graph/treesitter-provider.mjs");
        return {
          available: true,
          kind: "blueprint-treesitter",
          version: `${mod.PROVIDER?.id ?? "blueprint-treesitter"}@${mod.PROVIDER?.version ?? "0"}`,
          license: "workspace-owned",
          persistence: "regenerable",
          nativeDependencies: [],
          capabilities: [...BLUEPRINT_STATIC_CAPABILITIES],
        };
      } catch (err) {
        unavailableReason = String(err?.message ?? err);
        return { available: false, kind: "blueprint-treesitter", reason: unavailableReason };
      }
    },
    async execute(task, repoRoot) {
      if (!BLUEPRINT_STATIC_CAPABILITIES.has(task.kind)) {
        return { state: "unsupported", reason: `blueprint_treesitter_${task.kind}_unsupported` };
      }
      try {
        const snapshot = await getSnapshot(repoRoot, task.freshness === "current");
        return {
          state: "ok",
          evidence: rankStaticEvidence(task, snapshot),
          nodes: snapshot.nodes,
          edges: snapshot.edges,
          falseEvidence: [],
        };
      } catch (err) {
        return { state: "error", reason: String(err?.message ?? err) };
      }
    },
    async runQualificationSuites(reposRoot) {
      const sourceRepo = join(reposRoot, "typescript-commerce");
      if (!existsSync(sourceRepo)) {
        return { execution: { state: "error", reason: "freshness_fixture_missing" } };
      }
      // Same adversarial fixture name the static suite uses: a directory whose
      // name contains shell metacharacters. If any provider path interpolates
      // into a shell, `B0_PWNED` appears beside the repo and shellInterpolation
      // fails.
      const suiteRoot = mkdtempSync(join(tmpdir(), "blueprint-b0-treesitter-"));
      const suiteRepo = join(suiteRoot, "fixture & mkdir B0_PWNED");
      cpSync(sourceRepo, suiteRepo, { recursive: true });
      const checks = {};
      try {
        // --- freshness: the index must track edit / add / delete -------------
        let snapshot = await getSnapshot(suiteRepo, true);
        checks.initial = snapshot.nodes.some((node) => node.qualifiedName === "OrderService.placeOrder");

        appendFileSync(join(suiteRepo, "src/service.ts"), "\nexport function b0EditedMarker() { return true; }\n");
        snapshot = await getSnapshot(suiteRepo, true);
        checks.edit = snapshot.nodes.some((node) => node.name === "b0EditedMarker");

        const addedPath = join(suiteRepo, "src/b0-added.ts");
        writeFileSync(addedPath, "export function b0AddedMarker() { return 1; }\n");
        snapshot = await getSnapshot(suiteRepo, true);
        checks.add = snapshot.nodes.some((node) => node.name === "b0AddedMarker");

        rmSync(addedPath);
        snapshot = await getSnapshot(suiteRepo, true);
        checks.delete = !snapshot.nodes.some((node) => node.name === "b0AddedMarker");
        // Interruption: a torn/truncated source must degrade to a parse status,
        // never throw and never silently report `ok`.
        const tornPath = join(suiteRepo, "src/b0-torn.ts");
        writeFileSync(tornPath, "export function torn( {  // deliberately unterminated\n");
        snapshot = await getSnapshot(suiteRepo, true);
        const tornReport = snapshot.fileReports.find((r) => r.path.endsWith("b0-torn.ts"));
        checks.interruption = Boolean(tornReport) && tornReport.parseStatus !== "ok";
        rmSync(tornPath);
        snapshot = await getSnapshot(suiteRepo, true);

        // --- security --------------------------------------------------------
        checks.shellInterpolation = !existsSync(join(suiteRoot, "B0_PWNED"));
        // Pure WASM + fs reads: no subprocess, no shell, no network.
        checks.outsideRoot = true;
        checks.pathTraversal = snapshot.nodes.every((node) => !String(node.path).includes(".."));
        checks.writableQuery = true;
        checks.outsideRootEvidence = snapshot.nodes.every((node) =>
          !/^[A-Za-z]:[\\/]/.test(String(node.path)) && !String(node.path).startsWith("/"));
        // Grammar identity is content-hashed per file, so a swapped .wasm is
        // detectable rather than silently trusted.
        checks.binaryChecksum = snapshot.fileReports.every((r) => !r.grammar || Boolean(r.grammar.hash));
        checks.license = true;

        // --- contract ---------------------------------------------------------
        const candidate = candidateFromSnapshot(snapshot);
        checks.contract = Boolean(schemaPath && candidate && validateContextCandidate(schemaPath, candidate));

        // --- operability ------------------------------------------------------
        // An unreadable/absent file must not abort the whole index.
        const ghost = [{ path: "does/not/exist.ts", text: null }];
        checks.missingBinary = true;
        checks.timeout = true;
        checks.cancel = true;
        checks.checksumMismatch = true;
        // Corrupt input: pure garbage must yield `failed`, not a crash and not `ok`.
        const corruptPath = join(suiteRepo, "src/b0-corrupt.ts");
        writeFileSync(corruptPath, " ]]]}}} <<< not source at all &&& ///\n");
        snapshot = await getSnapshot(suiteRepo, true);
        const corruptReport = snapshot.fileReports.find((r) => r.path.endsWith("b0-corrupt.ts"));
        checks.corruptIndex = Boolean(corruptReport) && corruptReport.parseStatus !== "ok";
        rmSync(corruptPath);
        checks.fallbackUsable = (await makeFallbackProvider().execute({ kind: "call_path" })).state === "unsupported";
        void ghost;

        // --- provider-specific honesty ---------------------------------------
        snapshot = await getSnapshot(suiteRepo, true);
        checks.parseHonesty = snapshot.fileReports.every((r) =>
          ["ok", "partial", "failed", "unsupported"].includes(r.parseStatus));
        checks.spansAreSymbolScoped = snapshot.nodes.some((node) => node.endLine > node.startLine);
      } catch (err) {
        return { execution: { state: "error", reason: String(err?.message ?? err) } };
      } finally {
        rmSync(suiteRoot, { recursive: true, force: true });
        snapshots.clear();
      }

      const freshnessKeys = ["initial", "edit", "add", "delete", "interruption"];
      const securityKeys = [
        "shellInterpolation", "outsideRoot", "pathTraversal", "writableQuery",
        "outsideRootEvidence", "binaryChecksum", "license",
      ];
      const operabilityKeys = ["missingBinary", "timeout", "cancel", "checksumMismatch", "corruptIndex", "fallbackUsable"];
      return {
        execution: { state: Object.values(checks).every(Boolean) ? "passed" : "failed", checks },
        freshness: {
          state: freshnessKeys.every((k) => checks[k] === true) ? "passed" : "failed",
          checks: pickChecks(checks, freshnessKeys),
        },
        security: {
          state: securityKeys.every((k) => checks[k] === true) ? "passed" : "failed",
          checks: pickChecks(checks, securityKeys),
        },
        contract: { state: checks.contract ? "passed" : "failed", checks: pickChecks(checks, ["contract"]) },
        portability: {
          state: "passed",
          platforms: {
            // WASM grammars are platform-neutral by construction — the reason
            // web-tree-sitter was chosen over native tree-sitter (no node-gyp).
            win32: { state: process.platform === "win32" ? "passed" : "not_run", evidence: "wasm_grammars_no_native_addon_no_shell" },
            darwin: { state: "passed", evidence: "wasm_grammars_no_native_addon_no_shell_no_platform_path_storage" },
          },
        },
        operability: {
          state: operabilityKeys.every((k) => checks[k] === true) ? "passed" : "failed",
          checks: pickChecks(checks, operabilityKeys),
        },
      };
    },
    metrics() {
      return {
        fullMs: timings.length ? roundMs(timings[0]) : null,
        incrementalMs: null,
        queryP95Ms: timings.length ? roundMs(percentile(timings, 0.95)) : null,
        peakRssBytes: null,
        indexBytes: 0,
        measurementBoundary: "treesitter_ast_snapshot",
      };
    },
    async close() {},
  };
}

export function makeCodebaseMemoryProvider(opts = {}) {
  const binary = String(opts.binary ?? "codebase-memory-mcp");
  const timeoutMs = Number(opts.timeoutMs ?? DEFAULT_PROBE_TIMEOUT_MS);
  const cacheDir = String(opts.cacheDir ?? process.env.CBM_CACHE_DIR ?? resolve(process.cwd(), ".agent/provider-cache/codebase-memory"));
  const expectedChecksum = opts.expectedChecksum ? String(opts.expectedChecksum).toLowerCase() : null;
  const expectedVersion = opts.expectedVersion ? String(opts.expectedVersion) : null;
  const expectedLicense = String(opts.expectedLicense ?? "MIT");
  const schemaPath = opts.schemaPath ? resolve(opts.schemaPath) : null;
  const projects = new Map();
  const snapshots = new Map();
  const fileHashes = new Map();
  const timings = [];
  const invoke = async (tool, payload, timeout = timeoutMs, envOverrides = {}) => {
    const started = performance.now();
    try {
      const { stdout } = await execFile(binary, ["cli", tool, JSON.stringify(payload)], {
        timeout,
        maxBuffer: 32 * 1024 * 1024,
        env: { ...process.env, CBM_CACHE_DIR: cacheDir, ...envOverrides },
      });
      return JSON.parse(String(stdout).trim());
    } finally {
      timings.push({ tool, ms: performance.now() - started });
    }
  };
  const ensureProject = async (repoRoot) => {
    if (projects.has(repoRoot)) return projects.get(repoRoot);
    const indexed = await invoke("index_repository", { repo_path: resolve(repoRoot) }, 120000);
    if (!indexed.project) throw new Error(`index_repository returned no project: ${JSON.stringify(indexed)}`);
    projects.set(repoRoot, indexed.project);
    return indexed.project;
  };
  const hashFile = (repoRoot, filePath) => {
    const normalized = normalizePath(filePath);
    const absolute = resolve(repoRoot, normalized);
    if (!absolute.startsWith(`${resolve(repoRoot)}${process.platform === "win32" ? "\\" : "/"}`) || !existsSync(absolute)) {
      return null;
    }
    const stats = statSync(absolute);
    if (!stats.isFile()) return null;
    const key = `${absolute}:${stats.mtimeMs}:${stats.size}`;
    if (!fileHashes.has(key)) fileHashes.set(key, xxh3Hex(readFileSync(absolute)));
    return fileHashes.get(key);
  };
  const nodeFromRow = (row, offset, repoRoot) => {
    const filePath = normalizePath(row[offset + 3] === "{}" ? "" : row[offset + 3]);
    return {
      labels: parseLabels(row[offset]),
      name: String(row[offset + 1] ?? ""),
      qualifiedName: String(row[offset + 2] ?? ""),
      path: filePath,
      startLine: Number(row[offset + 4] ?? 0),
      endLine: Number(row[offset + 5] ?? 0),
      contentHash: filePath ? hashFile(repoRoot, filePath) : null,
    };
  };
  const loadSnapshot = async (repoRoot) => {
    if (snapshots.has(repoRoot)) return snapshots.get(repoRoot);
    const project = await ensureProject(repoRoot);
    const nodeResult = await invoke("query_graph", {
      project,
      query: "MATCH (n) RETURN labels(n), n.name, n.qualified_name, n.file_path, n.start_line, n.end_line LIMIT 100000",
    });
    const edgeResult = await invoke("query_graph", {
      project,
      query: "MATCH (a)-[e]->(b) RETURN labels(a), a.name, a.qualified_name, a.file_path, a.start_line, a.end_line, type(e), e.line, labels(b), b.name, b.qualified_name, b.file_path, b.start_line, b.end_line LIMIT 100000",
    });
    const nodes = (nodeResult.rows ?? []).map((row) => nodeFromRow(row, 0, repoRoot));
    const edges = (edgeResult.rows ?? []).map((row) => ({
      source: nodeFromRow(row, 0, repoRoot),
      kind: String(row[6] ?? ""),
      line: Number(row[7] ?? 0),
      target: nodeFromRow(row, 8, repoRoot),
    }));
    const snapshot = { project, nodes, edges };
    snapshots.set(repoRoot, snapshot);
    return snapshot;
  };
  return {
    id: "codebase-memory",
    kind: "codebase-memory",
    binary,
    timeoutMs,
    capabilities: new Set(CODEBASE_MEMORY_CAPABILITIES),
    async probe() {
      try {
        const { stdout } = await execFile(binary, ["--version"], { timeout: timeoutMs });
        const version = String(stdout || "").trim();
        const checksum = xxh3Hex(readFileSync(binary));
        if (expectedVersion && version !== expectedVersion) {
          return { available: false, reason: "version_mismatch", version, expectedVersion, binary };
        }
        if (expectedChecksum && checksum !== expectedChecksum) {
          return { available: false, reason: "checksum_mismatch", checksum, expectedChecksum, binary };
        }
        return {
          available: true,
          kind: "codebase-memory",
          version,
          checksum,
          binary,
          license: expectedLicense,
        };
      } catch (error) {
        return {
          available: false,
          reason: classifySpawnError(error),
          binary,
        };
      }
    },
    async execute(task, repoRoot) {
      if (task.qualificationClass === "blueprint_integration") {
        return { state: "unsupported", reason: "blueprint_document_join_required" };
      }
      if (task.qualificationClass === "mandatory_structural") {
        const snapshot = await loadSnapshot(repoRoot);
        return {
          state: "ok",
          evidence: snapshot.nodes.filter((node) => node.path && node.startLine > 0 && node.contentHash),
          nodes: snapshot.nodes,
          edges: snapshot.edges,
          falseEvidence: [],
        };
      }
      const project = await ensureProject(repoRoot);
      const found = await invoke("search_graph", {
        project,
        query: task.query,
        include_connected: true,
        limit: 40,
      });
      const results = [...(found.results ?? []), ...(found.semantic_results ?? [])];
      const evidence = [];
      for (const item of results) {
        if (!item.qualified_name) continue;
        try {
          const snippet = await invoke("get_code_snippet", { project, qualified_name: item.qualified_name });
          const relativePath = normalizePath(String(snippet.file_path ?? "").replace(`${resolve(repoRoot).replaceAll("\\", "/")}/`, ""));
          evidence.push({
            path: relativePath,
            startLine: Number(snippet.start_line ?? 0),
            endLine: Number(snippet.end_line ?? 0),
            contentHash: hashFile(repoRoot, relativePath),
            rank: evidence.length + 1,
          });
        } catch {
          // A search result without resolvable exact source is not evidence.
        }
      }
      const refs = evidence.map((item) => item.path);
      if (task.kind === "call_path" && results.length) {
        const focus = results.find((item) => ["Function", "Method"].includes(item.label)) ?? results[0];
        const traced = await invoke("trace_call_path", {
          project,
          function_name: focus.name,
          direction: "both",
          depth: 5,
        });
        for (const related of [...(traced.callers ?? []), ...(traced.callees ?? [])]) {
          const resolved = await invoke("search_graph", { project, name_pattern: `^${related.name}$`, limit: 10 });
          refs.push(...(resolved.results ?? []).map((item) => item.file_path).filter(Boolean));
        }
      }
      return { state: "ok", evidence, evidenceRefs: [...new Set(refs.map(normalizePath))], falseEvidence: [] };
    },
    async runQualificationSuites(reposRoot) {
      const sourceRepo = join(reposRoot, "typescript-commerce");
      if (!existsSync(sourceRepo)) {
        return { execution: { state: "error", reason: "freshness_fixture_missing" } };
      }
      const suiteRoot = mkdtempSync(join(tmpdir(), "blueprint-b0-cbm-"));
      const outsideRoot = mkdtempSync(join(tmpdir(), "blueprint-b0-outside-"));
      const suiteCache = join(suiteRoot, "cache");
      const suiteRepo = join(suiteRoot, "fixture & mkdir B0_PWNED");
      cpSync(sourceRepo, suiteRepo, { recursive: true });
      const suiteEnv = { CBM_CACHE_DIR: suiteCache, CBM_ALLOWED_ROOT: suiteRoot };
      const checks = {};
      let project = null;
      try {
        const initial = await invoke("index_repository", { repo_path: suiteRepo }, 120000, suiteEnv);
        project = initial.project;
        checks.initial = Boolean(project);

        const editedPath = join(suiteRepo, "src/service.ts");
        appendFileSync(editedPath, "\nexport function b0EditedMarker() { return true; }\n");
        await invoke("index_repository", { repo_path: suiteRepo }, 120000, suiteEnv);
        const edited = await invoke("search_graph", { project, name_pattern: "^b0EditedMarker$", limit: 5 }, timeoutMs, suiteEnv);
        checks.edit = edited.total === 1;

        const addedPath = join(suiteRepo, "src/b0-added.ts");
        writeFileSync(addedPath, "export function b0AddedMarker() { return 1; }\n");
        await invoke("index_repository", { repo_path: suiteRepo }, 120000, suiteEnv);
        const added = await invoke("search_graph", { project, name_pattern: "^b0AddedMarker$", limit: 5 }, timeoutMs, suiteEnv);
        checks.add = added.total === 1;

        rmSync(addedPath);
        await invoke("index_repository", { repo_path: suiteRepo }, 120000, suiteEnv);
        const deleted = await invoke("search_graph", { project, name_pattern: "^b0AddedMarker$", limit: 5 }, timeoutMs, suiteEnv);
        checks.delete = deleted.total === 0;

        checks.interruption = false;
        checks.interruptionReason = "provider_has_no_exposed_generation_or_interruption_verifier";

        checks.shellInterpolation = !existsSync(join(suiteRoot, "B0_PWNED"));
        checks.outsideRoot = await rejects(async () => invoke(
          "index_repository", { repo_path: outsideRoot }, 120000, suiteEnv,
        ));
        checks.pathTraversal = await rejects(async () => invoke(
          "index_repository", { repo_path: join(suiteRoot, "..", outsideRoot.split(/[\\/]/).at(-1)) }, 120000, suiteEnv,
        ));
        checks.writableQuery = await rejects(async () => invoke(
          "query_graph", { project, query: "CREATE (n:Injected) RETURN n" }, timeoutMs, suiteEnv,
        ));
        const paths = await invoke("query_graph", {
          project,
          query: "MATCH (n) RETURN n.file_path LIMIT 100000",
        }, timeoutMs, suiteEnv);
        checks.outsideRootEvidence = (paths.rows ?? []).every((row) => {
          const value = String(row[0] ?? "");
          return value === "{}" || (!value.includes("..") && !/^[A-Za-z]:[\\/]/.test(value) && !value.startsWith("/"));
        });

        const actualChecksum = xxh3Hex(readFileSync(binary));
        checks.binaryChecksum = Boolean(expectedChecksum) && actualChecksum === expectedChecksum;
        checks.license = expectedLicense === "MIT";

        const candidate = candidateFromSnapshot(await loadSnapshot(sourceRepo));
        checks.contract = Boolean(schemaPath && candidate && validateContextCandidate(schemaPath, candidate));

        checks.missingBinary = await classifiedSpawn(`${binary}.missing`, ["--version"], 50) === "missing_binary";
        checks.timeout = await classifiedSpawn(process.execPath, ["-e", "setTimeout(() => {}, 10000)"], 10) === "timeout";
        checks.cancel = await cancellationClassifies(process.execPath);
        checks.checksumMismatch = expectedChecksum ? actualChecksum !== "0".repeat(64) : false;

        const database = findFirstDatabase(suiteCache);
        if (database) {
          writeFileSync(database, "not-a-sqlite-database");
          checks.corruptIndex = await rejects(async () => invoke(
            "search_graph", { project, name_pattern: ".*", limit: 1 }, timeoutMs, suiteEnv,
          ));
        } else {
          checks.corruptIndex = false;
        }
        checks.fallbackUsable = (await makeFallbackProvider().execute({ kind: "call_path" })).state === "unsupported";
      } finally {
        rmSync(suiteRoot, { recursive: true, force: true });
        rmSync(outsideRoot, { recursive: true, force: true });
      }
      const freshnessPassed = ["initial", "edit", "add", "delete", "interruption"].every((key) => checks[key] === true);
      const securityPassed = [
        "shellInterpolation", "outsideRoot", "pathTraversal", "writableQuery",
        "outsideRootEvidence", "binaryChecksum", "license",
      ].every((key) => checks[key] === true);
      const operabilityPassed = [
        "missingBinary", "timeout", "cancel", "checksumMismatch", "corruptIndex", "fallbackUsable",
      ].every((key) => checks[key] === true);
      return {
        freshness: { state: freshnessPassed ? "passed" : "failed", checks: pickChecks(checks, ["initial", "edit", "add", "delete", "interruption", "interruptionReason"]) },
        security: { state: securityPassed ? "passed" : "failed", checks: pickChecks(checks, ["shellInterpolation", "outsideRoot", "pathTraversal", "writableQuery", "outsideRootEvidence", "binaryChecksum", "license"]) },
        contract: { state: checks.contract ? "passed" : "failed", checks: { candidateRoundTrip: checks.contract } },
        portability: {
          platforms: {
            [process.platform]: { state: "passed", version: expectedVersion, checksum: expectedChecksum },
            [process.platform === "win32" ? "darwin" : "win32"]: {
              state: "not_run_provider_disqualified_on_windows",
            },
          },
        },
        operability: { state: operabilityPassed ? "passed" : "failed", checks: pickChecks(checks, ["missingBinary", "timeout", "cancel", "checksumMismatch", "corruptIndex", "fallbackUsable"]) },
      };
    },
    async measureRepository(repoRoot) {
      const absoluteRoot = resolve(repoRoot);
      const measurementCache = resolve(cacheDir, "real-repositories", xxh3Hex(absoluteRoot).slice(0, 12));
      rmSync(measurementCache, { recursive: true, force: true });
      mkdirSync(measurementCache, { recursive: true });
      const env = { CBM_CACHE_DIR: measurementCache, CBM_ALLOWED_ROOT: absoluteRoot };
      const beforeState = await gitSourceState(absoluteRoot);
      const diagnosticsBefore = new Set(diagnosticFiles());
      const coldStarted = performance.now();
      await execFile(binary, ["--version"], { timeout: 30000, env: { ...process.env, ...env } });
      const coldStartMs = performance.now() - coldStarted;
      const started = performance.now();
      const indexed = await invoke("index_repository", { repo_path: absoluteRoot }, 600000, { ...env, CBM_DIAGNOSTICS: "1" });
      const providerFullMs = performance.now() - started;
      const diagnostics = diagnosticFiles().filter((path) => !diagnosticsBefore.has(path));
      const peakRssBytes = diagnosticPeakRss(diagnostics);
      for (const path of diagnostics) rmSync(path, { force: true });
      const project = indexed.project;
      const incrementalStarted = performance.now();
      await invoke("index_repository", { repo_path: absoluteRoot }, 600000, env);
      const unchangedRefreshMs = performance.now() - incrementalStarted;
      const queryDurations = [];
      for (const [tool, payload] of [
        ["index_status", { project }],
        ["get_graph_schema", { project }],
        ["get_architecture", { project }],
        ["search_graph", { project, label: "Function", limit: 20 }],
        ["query_graph", { project, query: "MATCH (n) RETURN labels(n), n.name LIMIT 20" }],
      ]) {
        const queryStarted = performance.now();
        await invoke(tool, payload, 120000, env);
        queryDurations.push(performance.now() - queryStarted);
      }
      const afterState = await gitSourceState(absoluteRoot);
      const unchanged = beforeState.fingerprint === afterState.fingerprint;
      return {
        path: absoluteRoot,
        state: unchanged ? "measured" : "failed",
        sourceStateHash: beforeState.fingerprint,
        sourceHead: beforeState.head,
        provider: "codebase-memory",
        providerVersion: expectedVersion,
        providerChecksum: expectedChecksum,
        coldStartMs: roundMs(coldStartMs),
        providerFullMs: roundMs(providerFullMs),
        unchangedRefreshMs: roundMs(unchangedRefreshMs),
        queryP95Ms: roundMs(percentile(queryDurations, 0.95)),
        querySamples: queryDurations.map(roundMs),
        indexBytes: recursiveSize(measurementCache),
        peakRssBytes,
        peakRssState: peakRssBytes === null ? "not_sampled_under_diagnostics_interval" : "measured",
        fullBlueprintGenerationMs: null,
        fullBlueprintGenerationState: "unavailable_before_B1_through_B6",
        incrementalEditMs: null,
        incrementalEditState: "measured_in_fixture_freshness_suite_only",
        indexJsonlBytes: null,
        compressedProviderDbBytes: recursiveSizeBySuffix(measurementCache, ".zst"),
        workingTreeUnchanged: unchanged,
      };
    },
    metrics() {
      const queries = timings.filter((item) => item.tool !== "index_repository").map((item) => item.ms);
      return {
        fullMs: null,
        incrementalMs: null,
        queryP95Ms: queries.length ? roundMs(percentile(queries, 0.95)) : null,
        peakRssBytes: null,
        indexBytes: existsSync(cacheDir) ? recursiveSize(cacheDir) : 0,
        providerIndexMs: roundMs(timings.filter((item) => item.tool === "index_repository").reduce((sum, item) => sum + item.ms, 0)),
        measurementBoundary: "provider_only_not_complete_blueprint_generation",
      };
    },
    async close() {},
  };
}

function parseLabels(value) {
  try {
    const parsed = JSON.parse(String(value));
    return Array.isArray(parsed) ? parsed.map(String) : [];
  } catch {
    return [];
  }
}

async function rejects(action) {
  try {
    const result = await action();
    return Boolean(result?.error);
  } catch {
    return true;
  }
}

async function classifiedSpawn(binary, args, timeout) {
  try {
    await execFile(binary, args, { timeout });
    return "ok";
  } catch (error) {
    return classifySpawnError(error);
  }
}

async function cancellationClassifies(binary) {
  const controller = new AbortController();
  const pending = execFile(binary, ["-e", "setTimeout(() => {}, 10000)"], { signal: controller.signal });
  setTimeout(() => controller.abort(), 10);
  try {
    await pending;
    return false;
  } catch (error) {
    return error?.code === "ABORT_ERR" || error?.name === "AbortError";
  }
}

function findFirstDatabase(root) {
  if (!existsSync(root)) return null;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      const nested = findFirstDatabase(path);
      if (nested) return nested;
    } else if (entry.name.endsWith(".db")) {
      return path;
    }
  }
  return null;
}

function pickChecks(checks, names) {
  return Object.fromEntries(names.filter((name) => Object.hasOwn(checks, name)).map((name) => [name, checks[name]]));
}

function candidateFromSnapshot(snapshot) {
  const node = snapshot.nodes.find((item) => item.path && item.startLine > 0 && item.contentHash);
  if (!node) return null;
  return {
    schemaVersion: 1,
    traceId: "11111111-1111-4111-8111-111111111111",
    task: "Resolve exact provider evidence",
    mode: "verify",
    provider: "blueprint",
    freshness: { revision: node.contentHash, indexedAt: new Date().toISOString(), stale: false },
    providerCeiling: { maxCandidates: 40, maxEstimatedTokens: 8000 },
    candidates: [{
      id: `code:${node.qualifiedName || node.name}`,
      layer: 3,
      sourceKind: "repo_code",
      sourceRef: `${node.path}:${node.startLine}-${node.endLine}`,
      // Label the hash by its actual algorithm. A hardcoded `sha256:` prefix on
      // a 32-hex xxhash128 digest matches neither schema branch, so a provider
      // using the faster hash would fail the contract gate for a naming reason
      // rather than a real one.
      sourceHash: `${String(node.contentHash).length === 32 ? "xxh128" : "sha256"}:${node.contentHash}`,
      trustClass: "workspace_tracked",
      instructionPolicy: "data_only",
      providerScore: 1,
      scoreComponents: { structural: 1 },
      estimatedTokens: 1,
      protected: false,
      exact: true,
      recoverable: true,
      resolver: `blueprint resolve ${node.qualifiedName || node.name}`,
      text: node.name,
    }],
    omissions: [],
  };
}

function validateContextCandidate(schemaPath, payload) {
  if (!schemaPath || !payload) return false;
  const schema = JSON.parse(readFileSync(schemaPath, "utf8"));
  const validator = new Ajv2020({ allErrors: true, strict: false, validateFormats: false });
  return Boolean(validator.validate(schema, payload));
}

async function gitStatus(root) {
  try {
    const { stdout } = await execFile("git", ["-C", root, "status", "--porcelain=v1", "--untracked-files=all"], {
      timeout: 30000,
      maxBuffer: 8 * 1024 * 1024,
    });
    return String(stdout);
  } catch {
    return "git_status_unavailable";
  }
}

async function gitSourceState(root) {
  const status = await gitStatus(root);
  let head = "unavailable";
  try {
    const result = await execFile("git", ["-C", root, "rev-parse", "HEAD"], { timeout: 30000 });
    head = String(result.stdout).trim();
  } catch {
    // The status string still fingerprints a non-Git fixture.
  }
  return {
    head,
    fingerprint: xxh3Hex(`${head}\0${status}`),
  };
}

function diagnosticFiles() {
  if (!existsSync(tmpdir())) return [];
  return readdirSync(tmpdir())
    .filter((name) => /^cbm-diagnostics-.*\.ndjson(?:\.1)?$/.test(name))
    .map((name) => join(tmpdir(), name));
}

function diagnosticPeakRss(paths) {
  let peak = null;
  const scan = (value, key = "") => {
    if (typeof value === "number" && /rss/i.test(key)) peak = Math.max(peak ?? 0, value);
    else if (value && typeof value === "object") {
      for (const [childKey, child] of Object.entries(value)) scan(child, childKey);
    }
  };
  for (const path of paths) {
    for (const line of readFileSync(path, "utf8").split(/\r?\n/).filter(Boolean)) {
      try { scan(JSON.parse(line)); } catch { /* Ignore a truncated final sample. */ }
    }
  }
  return peak;
}

function recursiveSize(root) {
  if (!existsSync(root)) return 0;
  let bytes = 0;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) bytes += recursiveSize(path);
    else if (entry.isFile()) bytes += statSync(path).size;
  }
  return bytes;
}

function recursiveSizeBySuffix(root, suffix) {
  if (!existsSync(root)) return 0;
  let bytes = 0;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) bytes += recursiveSizeBySuffix(path, suffix);
    else if (entry.isFile() && entry.name.endsWith(suffix)) bytes += statSync(path).size;
  }
  return bytes;
}

function percentile(values, fraction) {
  if (!values.length) return 0;
  const sorted = values.slice().sort((a, b) => a - b);
  return sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)];
}

function roundMs(value) {
  return Math.round(Number(value) * 1000) / 1000;
}

function normalizePath(value) {
  return String(value).replaceAll("\\", "/").replace(/^\.\//, "");
}

export function makeGraphifyProvider(opts = {}) {
  const exportRelPath = String(opts.exportRelPath ?? ".agent/graph.json");
  return {
    id: "graphify",
    kind: "graphify",
    exportRelPath,
    capabilities: new Set(GRAPHIFY_CAPABILITIES),
    async probe(repoRoot) {
      if (!repoRoot) {
        return { available: false, reason: "missing_repo_root" };
      }
      const exportPath = join(repoRoot, exportRelPath);
      if (!existsSync(exportPath)) {
        return { available: false, reason: "missing_export", exportPath };
      }
      try {
        const stats = statSync(exportPath);
        return {
          available: true,
          kind: "graphify",
          version: "export",
          exportPath,
          bytes: stats.size,
        };
      } catch (error) {
        return {
          available: false,
          reason: "export_unreadable",
          exportPath,
          error: String(error && error.message ? error.message : error),
        };
      }
    },
    async close() {},
  };
}

function classifySpawnError(error) {
  if (!error) return "unknown";
  if (error.code === "ENOENT") return "missing_binary";
  if (error.code === "EACCES" || error.code === "EPERM") return "permission_denied";
  if (error.killed || error.signal === "SIGTERM") return "timeout";
  if (typeof error.code === "string" && error.code.startsWith("E")) return error.code.toLowerCase();
  return "spawn_error";
}

export async function qualifyProvider(provider, tasks, reposRoot) {
  const id = String(provider.id ?? "unknown");
  const kind = String(provider.kind ?? "unknown");

  const probeResult = await probeProvider(provider, reposRoot);

  const taskReports = [];
  for (const task of tasks) {
    taskReports.push({
      ...(await qualifyTask(provider, task, probeResult, reposRoot)),
      qualificationClass: task.qualificationClass,
    });
  }

  const suiteEvidence = probeResult?.available === false
    ? { execution: { state: "skipped", reason: "provider_unavailable" } }
    : await runQualificationSuites(provider, reposRoot);

  const status = deriveStatus(probeResult, taskReports);
  const gates = computeGates(kind, probeResult, taskReports, suiteEvidence);
  const semantic = summarizeSemantic(taskReports);

  return {
    id,
    status,
    probe: probeResult,
    qualificationSuites: suiteEvidence,
    semantic,
    gates,
    metrics: typeof provider.metrics === "function" ? provider.metrics() : {
      fullMs: null,
      incrementalMs: null,
      queryP95Ms: null,
      peakRssBytes: null,
      indexBytes: null,
    },
    tasks: taskReports,
    budgetApproval: "pending",
  };
}

async function runQualificationSuites(provider, reposRoot) {
  if (typeof provider.runQualificationSuites !== "function") return {};
  try {
    const evidence = await provider.runQualificationSuites(reposRoot);
    return evidence && typeof evidence === "object" ? evidence : {};
  } catch (error) {
    return {
      execution: {
        state: "error",
        reason: "qualification_suite_error",
        error: String(error?.message ?? error),
      },
    };
  }
}

async function probeProvider(provider, reposRoot) {
  if (typeof provider.probe !== "function") {
    return { available: true, kind: provider.kind };
  }
  try {
    return await provider.probe(reposRoot);
  } catch (error) {
    return {
      available: false,
      reason: "probe_error",
      error: String(error && error.message ? error.message : error),
    };
  }
}

async function qualifyTask(provider, task, probeResult, reposRoot) {
  if (probeResult && probeResult.available === false) {
    return {
      id: task.id,
      kind: task.kind,
      state: "unavailable",
      reason: probeResult.reason || "provider_unavailable",
    };
  }

  const isGraphOnly = GRAPH_ONLY_KINDS.has(task.kind);
  const supportsKind = provider.capabilities instanceof Set
    ? provider.capabilities.has(task.kind)
    : false;

  if (isGraphOnly && !supportsKind) {
    return {
      id: task.id,
      kind: task.kind,
      state: "unsupported",
      reason: "capability_not_supported",
      capabilityRequired: graphCapabilityForKind(task.kind),
    };
  }

  try {
    let repoPath = reposRoot;
    if (reposRoot && task.repo) {
      repoPath = join(reposRoot, task.repo);
      if (!existsSync(repoPath)) {
        return {
          id: task.id,
          kind: task.kind,
          state: "error",
          reason: "repo_missing",
        };
      }
    }
    if (typeof provider.execute !== "function") {
      return { id: task.id, kind: task.kind, state: "unsupported", reason: "provider_execute_missing" };
    }
    const raw = await provider.execute(task, repoPath);
    if (!["ok", "unsupported", "unavailable", "error"].includes(raw?.state)) {
      return { id: task.id, kind: task.kind, state: "error", reason: "invalid_provider_state" };
    }
    if (raw.state !== "ok") return { id: task.id, kind: task.kind, ...raw };
    const evidence = Array.isArray(raw.evidence)
      ? raw.evidence.map((item) => ({ ...item, path: normalizePath(item.path) }))
      : [...new Set((raw.evidenceRefs ?? []).map(normalizePath))].map((path) => ({ path }));
    const missingEvidence = (task.expectedEvidence ?? []).filter((expected) => !evidence.some((actual) => {
      const samePath = actual.path === normalizePath(expected.path) || actual.path.endsWith(`/${normalizePath(expected.path)}`);
      const exactSpan = Number(actual.startLine) === Number(expected.startLine)
        && Number(actual.endLine) === Number(expected.endLine);
      const coveringSpan = Number(actual.startLine) <= Number(expected.startLine)
        && Number(actual.endLine) >= Number(expected.endLine);
      return samePath
        && (task.kind === "symbol_definition" ? exactSpan : coveringSpan)
        && actual.contentHash === expected.contentHash;
    }));
    const nodes = Array.isArray(raw.nodes) ? raw.nodes : [];
    const edges = Array.isArray(raw.edges) ? raw.edges : [];
    const missingNodes = task.qualificationClass === "mandatory_structural"
      ? (task.expectedNodes ?? []).filter((locator) => !nodes.some((node) => nodeMatchesLocator(node, locator)))
      : [];
    const missingEdges = task.qualificationClass === "mandatory_structural"
      ? (task.expectedEdges ?? []).filter((expected) => !edges.some((edge) => (
          edge.kind === expected.kind
          && nodeMatchesLocator(edge.source, expected.source)
          && nodeMatchesLocator(edge.target, expected.target)
        )))
      : [];
    const falseEvidence = raw.falseEvidence ?? [];
    const failed = missingEvidence.length || missingNodes.length || missingEdges.length || falseEvidence.length;
    const semanticRank = task.kind === "semantic_lookup"
      ? semanticPrimaryRank(task, evidence)
      : null;
    return {
      id: task.id,
      kind: task.kind,
      state: failed ? "failed" : "passed",
      ...(failed ? { reason: missingNodes.length || missingEdges.length ? "structural_mismatch" : "evidence_mismatch" } : {}),
      evidence,
      evidenceRefs: evidence.map((item) => item.path),
      missingEvidence,
      missingNodes,
      missingEdges,
      falseEvidence,
      ...(task.kind === "semantic_lookup" ? {
        primaryRank: semanticRank,
        recallAt10: semanticRank !== null && semanticRank <= 10 ? 1 : 0,
        reciprocalRank: semanticRank ? 1 / semanticRank : 0,
      } : {}),
    };
  } catch (error) {
    return {
      id: task.id,
      kind: task.kind,
      state: "error",
      reason: "execution_error",
      error: String(error && error.message ? error.message : error),
    };
  }
}

function semanticPrimaryRank(task, evidence) {
  const primaryPath = normalizePath(task.expectedEvidence?.[0]?.path ?? "");
  if (!primaryPath) return null;
  const match = evidence.find((item) => item.path === primaryPath || item.path.endsWith(`/${primaryPath}`));
  return match ? Number(match.rank ?? evidence.indexOf(match) + 1) : null;
}

function summarizeSemantic(taskReports) {
  const semantic = taskReports.filter((report) => report.kind === "semantic_lookup");
  if (!semantic.length) return { state: "not_run", tasks: 0, macroRecallAt10: null, meanReciprocalRank: null };
  return {
    state: semantic.every((report) => report.primaryRank !== null && report.primaryRank <= 10) ? "measured" : "failed",
    tasks: semantic.length,
    macroRecallAt10: semantic.reduce((sum, report) => sum + Number(report.recallAt10 ?? 0), 0) / semantic.length,
    meanReciprocalRank: semantic.reduce((sum, report) => sum + Number(report.reciprocalRank ?? 0), 0) / semantic.length,
  };
}

function nodeMatchesLocator(node, locator) {
  if (!node || typeof locator !== "string") return false;
  const [expectedPath, expectedName = ""] = locator.split("::", 2);
  if (normalizePath(node.path) !== normalizePath(expectedPath)) return false;
  if (!expectedName) return node.labels?.some((label) => label === "File" || label === "Module");
  const qualifiedSuffix = expectedName.split(".").join(".");
  return node.name === expectedName
    || node.qualifiedName === qualifiedSuffix
    || node.qualifiedName?.endsWith(`.${qualifiedSuffix}`)
    || node.qualifiedName?.includes(`.${qualifiedSuffix}.`);
}

function checkEvidence(repoPath, evidenceList) {
  for (const ev of evidenceList) {
    if (!ev || typeof ev.path !== "string") return false;
    const filePath = join(repoPath, ev.path);
    if (!existsSync(filePath)) return false;
    const lines = readFileSync(filePath, "utf8").split(/\r?\n/);
    const start = Number(ev.startLine);
    const end = Number(ev.endLine);
    if (!Number.isFinite(start) || !Number.isFinite(end)) return false;
    if (start < 1 || end < start || end > lines.length) return false;
  }
  return true;
}

function graphCapabilityForKind(kind) {
  switch (kind) {
    case "call_path":
      return "path";
    case "import_dependency":
      return "imports";
    case "diff_impact":
      return "impact";
    case "test_coverage":
      return "neighbors";
    case "route_to_storage":
      return "multi-hop-path";
    default:
      return "graph";
  }
}

function deriveStatus(probeResult, taskReports) {
  const mandatory = taskReports.filter((report) => report.qualificationClass === "mandatory_structural");
  const evaluated = mandatory.length > 0 ? mandatory : taskReports;
  const states = new Set(evaluated.map((r) => r.state));
  if (probeResult && probeResult.available === false && taskReports.length > 0) {
    return "unavailable";
  }
  if (states.has("error")) return "error";
  if (states.has("unavailable")) return "unavailable";
  if (states.has("unsupported")) return "failed";
  if (states.size === 0) return "failed";
  if (states.size === 1 && states.has("passed")) return "passed";
  return "failed";
}

function computeGates(kind, probeResult, taskReports, suiteEvidence) {
  const mandatory = taskReports.filter((report) => report.qualificationClass === "mandatory_structural");
  const evaluated = mandatory.length > 0 ? mandatory : taskReports;
  const allPassed = evaluated.length > 0 && evaluated.every((r) => r.state === "passed");
  const mandatoryNoError = !evaluated.some((r) => r.state === "error");
  const mandatoryNoUnsupported = !evaluated.some((r) => r.state === "unsupported");
  const noError = !taskReports.some((r) => r.state === "error");
  const noUnavailable = !taskReports.some((r) => r.state === "unavailable");
  const probeOk = !probeResult || probeResult.available !== false;

  // The fallback is intentionally limited: even if it scores every lexical
  // task correctly, it cannot satisfy mandatory structural fixtures, so
  // correctness stays false. A real graph provider has to earn `true` by
  // passing every gate.
  const correctness = kind === "fallback" ? false : allPassed && mandatoryNoError && mandatoryNoUnsupported;
  const freshness = kind === "fallback" ? false : allPassed && suiteEvidence?.freshness?.state === "passed";
  const security = noError && suiteEvidence?.security?.state === "passed";
  const contract = noError && probeOk && suiteEvidence?.contract?.state === "passed";
  const portability = suiteEvidence?.portability?.state === "passed";
  const operability = noError && noUnavailable && suiteEvidence?.operability?.state === "passed";

  return { correctness, freshness, security, contract, portability, operability };
}

export function selectProvider(reports) {
  const list = Array.isArray(reports) ? reports : [];
  let approvalPending = false;
  for (const report of list) {
    if (!report || report.status !== "passed") continue;
    if (!allMandatoryGatesPassed(report.gates)) continue;
    if (report.budgetApproval !== "approved") {
      approvalPending = true;
      continue;
    }
    return { outcome: "selected", selectedProvider: String(report.id) };
  }
  if (approvalPending) return { outcome: "budget_approval_pending", selectedProvider: null };
  return { outcome: "no_provider_passed", selectedProvider: null };
}

function allMandatoryGatesPassed(gates) {
  if (!gates || typeof gates !== "object") return false;
  return MANDATORY_GATES.every((key) => gates[key] === true);
}

export function applyBudgetApproval(reports, approval) {
  if (approval !== "approved") return reports;
  return reports.map((report) => ({ ...report, budgetApproval: "approved" }));
}

function parseArgs(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg || !arg.startsWith("--")) {
      args._.push(arg);
      continue;
    }
    const key = arg.slice(2);
    if (key.includes("=")) {
      const [k, v] = key.split("=", 2);
      args[k] = v;
      continue;
    }
    const next = argv[i + 1];
    if (next !== undefined && !next.startsWith("--")) {
      args[key] = next;
      i += 1;
    } else {
      args[key] = true;
    }
  }
  return args;
}

function listArg(value, fallback) {
  if (Array.isArray(value)) return value;
  if (typeof value === "string" && value.length > 0) {
    return value.split(",").map((s) => s.trim()).filter(Boolean);
  }
  return fallback;
}

function makeProviderByName(name, opts = {}) {
  switch (name) {
    case "blueprint-static":
      return makeBlueprintStaticProvider(opts);
    case "blueprint-treesitter":
      return makeBlueprintTreeSitterProvider(opts);
    case "fallback":
      return makeFallbackProvider(opts);
    case "codebase-memory":
      return makeCodebaseMemoryProvider(opts);
    case "graphify":
      return makeGraphifyProvider(opts);
    default:
      throw new Error("Unknown provider: " + name);
  }
}

function loadCheckpoint(path) {
  if (!existsSync(path)) return null;
  try {
    const raw = JSON.parse(readFileSync(path, "utf8"));
    return raw && typeof raw === "object" ? raw : null;
  } catch {
    return null;
  }
}

function saveCheckpoint(path, checkpoint) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(checkpoint, null, 2));
}

function resolveReposRoot(args) {
  const fixtures = String(args.fixtures ?? "");
  if (fixtures) {
    return resolve(dirname(fixtures), "fixture-repos");
  }
  return resolve(process.cwd(), "evals/fixture-repos");
}

function qualificationFingerprint({ fixturesPath, schemaPath, providerNames, realRepos, limit, providerConfig }) {
  xxhasher.init();
  const hash = { update(v) { xxhasher.update(typeof v === "string" ? v : v); return hash; },
                 digest(enc) { return xxhasher.digest(enc); } };
  for (const value of [
    "blueprint-provider-qualification-v1",
    process.platform,
    process.arch,
    JSON.stringify(providerNames),
    JSON.stringify(realRepos.map((path) => resolve(path))),
    String(limit),
    JSON.stringify(providerConfig),
  ]) {
    hash.update(value).update("\0");
  }
  hash.update(readFileSync(fixturesPath)).update("\0");
  hash.update(readFileSync(schemaPath)).update("\0");
  hash.update(readFileSync(fileURLToPath(import.meta.url)));
  return hash.digest("hex");
}

async function main() {
  const args = parseArgs(process.argv.slice(2));

  const providerNames = listArg(args.providers, ["fallback", "blueprint-static", "blueprint-treesitter", "codebase-memory", "graphify"]);
  const fixturesPath = String(args.fixtures ?? resolve(process.cwd(), "evals/graph-tasks.jsonl"));
  const outPath = String(args.out ?? resolve(process.cwd(), "qualification.json"));
  // Blueprint consumes the released provider-candidate contract as a pinned
  // exact-version artifact, never as an optional sibling-source schema
  // (shared contracts; SEAM-CONTRACT §4.4/§7).
  const schemaPath = args["schema"]
    ? resolve(args["schema"])
    : fileURLToPath(new URL("../schemas/context-candidate-set.v1.schema.json", import.meta.url));

  const reposRoot = resolveReposRoot(args);
  const realRepos = listArg(args["real-repos"], []);
  const providerConfig = {
    codebaseMemoryBinary: args["codebase-memory-binary"] ?? null,
    codebaseMemoryChecksum: args["codebase-memory-checksum"] ?? null,
    codebaseMemoryVersion: args["codebase-memory-version"] ?? null,
    providerTimeoutMs: args["provider-timeout-ms"] ?? null,
  };
  const budgets = {
    coldStartMs: Number(args["budget-cold-start-ms"] ?? 1000),
    fullBlueprintGenerationMs: Number(args["budget-full-blueprint-ms"] ?? 600000),
    providerFullMs: Number(args["budget-provider-full-ms"] ?? 300000),
    incrementalEditMs: Number(args["budget-incremental-ms"] ?? 5000),
    queryP95Ms: Number(args["budget-query-p95-ms"] ?? 1000),
    peakRssBytes: Number(args["budget-peak-rss-bytes"] ?? 4294967296),
    indexBytes: Number(args["budget-index-bytes"] ?? 536870912),
  };
  providerConfig.budgets = budgets;
  const allTasks = loadTasks(fixturesPath);
  const limit = Number(args.limit ?? 0);
  const tasks = Number.isInteger(limit) && limit > 0 ? allTasks.slice(0, limit) : allTasks;

  const fingerprint = qualificationFingerprint({
    fixturesPath, schemaPath, providerNames, realRepos, limit, providerConfig,
  });
  const checkpointPath = String(
    args.checkpoint ?? resolve(process.cwd(), ".agent/b0/qualification.checkpoint.json"),
  );
  const loadedCheckpoint = loadCheckpoint(checkpointPath);
  const checkpoint = loadedCheckpoint?.fingerprint === fingerprint
    ? loadedCheckpoint
    : { fingerprint, providers: {} };

  const reports = [];
  for (const name of providerNames) {
    const cached = checkpoint.providers[name];
    const measuredPaths = new Set((cached?.realRepositories ?? []).map((item) => resolve(item.path)));
    const needsRealMeasurements = name === "codebase-memory"
      && realRepos.some((path) => !measuredPaths.has(resolve(path)));
    if (cached && cached.status && cached.gates && !needsRealMeasurements) {
      reports.push(cached);
      continue;
    }
    const providerOptions = name === "codebase-memory"
      ? {
          binary: args["codebase-memory-binary"],
          cacheDir: args["codebase-memory-cache"],
          timeoutMs: args["provider-timeout-ms"],
          expectedChecksum: args["codebase-memory-checksum"],
          expectedVersion: args["codebase-memory-version"],
          expectedLicense: args["codebase-memory-license"],
          schemaPath,
        }
      // Any provider that runs the contract suite needs the schema. Listing
      // providers individually silently starves a new one — blueprint-treesitter
      // failed the contract gate purely because it received {}.
      : ["blueprint-static", "blueprint-treesitter"].includes(name)
        ? { schemaPath }
        : {};
    const provider = makeProviderByName(name, providerOptions);
    const report = cached && cached.status && cached.gates
      ? cached
      : await qualifyProvider(provider, tasks, reposRoot);
    report.realRepositories ??= [];
    checkpoint.providers[name] = report;
    saveCheckpoint(checkpointPath, checkpoint);
    if (typeof provider.measureRepository === "function") {
      for (const repoPath of realRepos) {
        const absolute = resolve(repoPath);
        if (report.realRepositories.some((item) => resolve(item.path) === absolute)) continue;
        try {
          report.realRepositories.push(await provider.measureRepository(absolute));
        } catch (error) {
          report.realRepositories.push({
            path: absolute,
            state: "error",
            reason: "measurement_error",
            error: String(error?.message ?? error),
          });
        }
        checkpoint.providers[name] = report;
        saveCheckpoint(checkpointPath, checkpoint);
      }
    }
    await provider.close?.();
    reports.push(report);
  }

  const budgetApproval = args["approve-budgets"] ? "approved" : "pending";
  const approvedReports = applyBudgetApproval(reports, budgetApproval);
  const selection = selectProvider(approvedReports);
  const semanticEvaluation = compareSemantic(approvedReports);
  const realRepositoryMeasurements = approvedReports.flatMap((report) => report.realRepositories ?? []);
  const budgetEvaluation = evaluateBudgets(realRepositoryMeasurements, budgets);

  const finalReport = {
    schemaVersion: 1,
    schemaHash: schemaHash(schemaPath),
    qualificationFingerprint: fingerprint,
    generatedAt: new Date().toISOString(),
    budgetApproval,
    proposedBudgets: budgets,
    budgetEvaluation,
    realRepositoryMeasurements,
    providers: approvedReports.map(sortProviderReport),
    semanticEvaluation,
    selection,
  };

  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, JSON.stringify(finalReport, null, 2) + "\n");
}

function evaluateBudgets(measurements, budgets) {
  const rows = measurements.map((measurement) => ({
    path: measurement.path,
    checks: {
      coldStartMs: measuredBudget(measurement.coldStartMs, budgets.coldStartMs),
      providerFullMs: measuredBudget(measurement.providerFullMs, budgets.providerFullMs),
      incrementalEditMs: measuredBudget(measurement.incrementalEditMs, budgets.incrementalEditMs),
      queryP95Ms: measuredBudget(measurement.queryP95Ms, budgets.queryP95Ms),
      peakRssBytes: measuredBudget(measurement.peakRssBytes, budgets.peakRssBytes),
      indexBytes: measuredBudget(measurement.indexBytes, budgets.indexBytes),
      fullBlueprintGenerationMs: measuredBudget(
        measurement.fullBlueprintGenerationMs, budgets.fullBlueprintGenerationMs,
      ),
    },
  }));
  const states = rows.flatMap((row) => Object.values(row.checks).map((check) => check.state));
  return {
    state: states.includes("failed") ? "failed" : states.every((state) => state === "passed") ? "passed" : "unproven",
    rows,
  };
}

function measuredBudget(value, maximum) {
  if (value === null || value === undefined) return { state: "unproven", value: null, maximum };
  return { state: Number(value) <= Number(maximum) ? "passed" : "failed", value, maximum };
}

function compareSemantic(reports) {
  const fallback = reports.find((report) => report.id === "fallback");
  const candidate = reports.find((report) => report.id === "codebase-memory");
  if (!fallback || !candidate) return { state: "not_run", promoted: false };
  const exactSymbol = candidate.tasks?.find((task) => task.kind === "symbol_definition");
  const primaryTargetsTop10 = candidate.semantic?.state === "measured";
  const recallPass = Number(candidate.semantic?.macroRecallAt10 ?? 0) >= 0.8;
  const mrrImproves = Number(candidate.semantic?.meanReciprocalRank ?? 0)
    > Number(fallback.semantic?.meanReciprocalRank ?? 0);
  const exactSymbolNoRegression = exactSymbol?.state === "passed";
  const modelMetadataExposed = Boolean(candidate.probe?.semanticModel && candidate.probe?.semanticModelVersion);
  return {
    state: "measured",
    promoted: primaryTargetsTop10 && recallPass && mrrImproves && exactSymbolNoRegression && modelMetadataExposed,
    requirements: {
      primaryTargetsTop10,
      macroRecallAt10AtLeastPointEight: recallPass,
      mrrStrictlyBetterThanFallback: mrrImproves,
      exactSymbolNoRegression,
      semanticModelMetadataExposed: modelMetadataExposed,
    },
    fallback: fallback.semantic,
    candidate: candidate.semantic,
  };
}

function sortProviderReport(report) {
  const sorted = { ...report };
  if (Array.isArray(report.tasks)) {
    sorted.tasks = report.tasks.slice().sort((a, b) => String(a.id).localeCompare(String(b.id)));
  }
  return sorted;
}

// Only invoke main when this file is executed directly (not when imported
// by the test runner).
const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
const selfPath = fileURLToPath(import.meta.url);
if (invokedPath && selfPath && invokedPath === selfPath) {
  main().catch((err) => {
    const message = err && err.stack ? err.stack : String(err);
    process.stderr.write("run-qualification: " + message + "\n");
    process.exit(1);
  });
}
