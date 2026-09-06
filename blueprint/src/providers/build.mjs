import { isAbsolute, relative, resolve } from "node:path";

import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "../graph/confidence-tiers.mjs";
import { pythonScipProvider } from "./compilers/python-scip.mjs";
import { collectSemanticEvidenceSync } from "./semantic-orchestrator.mjs";
import { auditSourceDispositions } from "./source-disposition.mjs";
import { augmentStructuralIntelligence, STRUCTURAL_INTELLIGENCE_PROVIDER } from "../graph/structural-intelligence.mjs";
import { augmentFrameworkIntelligence, FRAMEWORK_INTELLIGENCE_PROVIDER } from "../graph/framework-intelligence.mjs";
import { attachPortableIdentities } from "../graph/portable-identity.mjs";
import { detectProjectConventions } from "../graph/conventions.mjs";
import { extractJavaScriptModuleSpecifiers, resolveModuleSpecifier } from "./modules/javascript.mjs";
import { extractPythonModuleSpecifiers, resolvePythonModule } from "./modules/python-resolver.mjs";
import {
  domainGateActive,
  extractDatabaseFacts,
  extractDeploymentFacts,
  extractEventFacts,
  extractRoutes,
  frameworkGateActive,
  importedPackages,
} from "./frameworks/index.mjs";
import { extractDockerfileFacts, extractSqlFacts, profileForPath } from "./schemas/sql.mjs";
import { extractTerraformFacts } from "./iac/terraform.mjs";
import { bridgeSeamProvider } from "./bridges/seams.mjs";

const MODULE_PROVIDER = Object.freeze({ id: "blueprint-modules", version: "exact-first-v2" });
const FRAMEWORK_PROVIDER = Object.freeze({ id: "blueprint-frameworks", version: "gated-evidence-v1" });
const SQL_PROVIDER = Object.freeze({ id: "blueprint-sql", version: "schema-facts-v1" });
const TERRAFORM_PROVIDER = Object.freeze({ id: "blueprint-terraform", version: "resource-facts-v1" });

function normalizePath(value) {
  return String(value).replaceAll("\\", "/").replace(/^\.\//, "");
}

function inside(root, target) {
  const rel = relative(root, target);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

function evidence(file, line, extra = {}) {
  return [{ path: file.path, startLine: line, endLine: line, contentHash: file.contentHash ?? null, ...extra }];
}

function safeId(value) {
  return String(value).replace(/[^A-Za-z0-9_.-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 100) || "fact";
}

function providerNode(provider, file, category, name, line, attributes = {}) {
  return {
    id: `domain:${provider.id}:${file.path}:${category}:${safeId(name)}:${line}`,
    kind: "domain",
    labels: [category],
    name,
    qualifiedName: `${file.path}:${category}:${name}`,
    path: file.path,
    confidence: 1,
    provider: provider.id,
    factProvider: provider,
    evidence: evidence(file, line, attributes),
    ...attributes,
  };
}

function providerEdge(provider, file, kind, target, line, attributes = {}) {
  const tier = attributes.confidenceTier ?? EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION;
  return {
    id: `edge:${kind}:file:${file.path}->${target}:${provider.id}:${line}`,
    kind,
    source: `file:${file.path}`,
    target,
    confidence: attributes.confidence ?? tierConfidence(tier),
    confidenceTier: tier,
    resolved: true,
    provider: provider.id,
    factProvider: provider,
    evidence: evidence(file, line, attributes),
    ...attributes,
  };
}

function moduleRecords(file) {
  const ext = file.path.split(".").at(-1)?.toLowerCase();
  if (["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"].includes(ext)) {
    return extractJavaScriptModuleSpecifiers(file.text).map((record) => ({ ...record, language: "javascript" }));
  }
  if (ext === "py") return extractPythonModuleSpecifiers(file.text).map((record) => ({ ...record, language: "python" }));
  return [];
}

function resolveModule(record, file, root) {
  const fromFile = resolve(root, file.path);
  return record.language === "python"
    ? resolvePythonModule({ specifier: record.specifier, importedName: record.importedName, fromFile, repoRoot: root })
    : resolveModuleSpecifier({ specifier: record.specifier, fromFile, repoRoot: root, isTypeScript: /\.[cm]?tsx?$/.test(file.path) });
}

function moduleCandidatePaths(result, root) {
  return (result.candidates ?? [])
    .filter((candidate) => inside(root, candidate))
    .map((candidate) => normalizePath(relative(root, candidate)))
    .sort((left, right) => left.localeCompare(right));
}

function abstainFromAmbiguousImport(generation, file, record, candidatePaths, ambiguityReason, protectedTargets) {
  const sourceId = `file:${file.path}`;
  const candidateTargets = new Set(candidatePaths.map((path) => `file:${path}`));
  let unresolvedEdge = generation.edges.find((edge) => edge.kind === "IMPORTS"
    && edge.source === sourceId
    && edge.target === null
    && edge.specifier === record.specifier);

  for (const edge of generation.edges) {
    if (edge.kind !== "IMPORTS" || edge.source !== sourceId || !candidateTargets.has(edge.target)) continue;
    if (protectedTargets.has(edge.target)) continue;
    edge.id = `edge:IMPORTS:${sourceId}->unresolved:${record.specifier}`;
    edge.target = null;
    edge.confidence = tierConfidence(EDGE_CONFIDENCE_TIERS.UNRESOLVED);
    edge.confidenceTier = EDGE_CONFIDENCE_TIERS.UNRESOLVED;
    edge.resolved = false;
    edge.specifier = record.specifier;
    edge.reason = ambiguityReason;
    edge.resolutionStatus = "AMBIGUOUS";
    edge.candidates = candidatePaths;
    unresolvedEdge ??= edge;
  }

  const sourceSymbols = new Set(generation.nodes
    .filter((node) => node.kind === "symbol" && node.path === file.path)
    .map((node) => node.id));
  const candidateSymbols = new Set(generation.nodes
    .filter((node) => node.kind === "symbol" && candidatePaths.includes(node.path))
    .map((node) => node.id));
  for (let index = generation.edges.length - 1; index >= 0; index -= 1) {
    const edge = generation.edges[index];
    if (!["CALLS", "TESTS"].includes(edge.kind)) continue;
    if (edge.confidenceTier !== EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC) continue;
    if (sourceSymbols.has(edge.source) && candidateSymbols.has(edge.target)) generation.edges.splice(index, 1);
  }

  if (unresolvedEdge) return unresolvedEdge;
  const edge = {
    id: `edge:IMPORTS:${sourceId}->unresolved:${record.specifier}:${MODULE_PROVIDER.id}:${record.line}`,
    kind: "IMPORTS",
    source: sourceId,
    target: null,
    confidence: tierConfidence(EDGE_CONFIDENCE_TIERS.UNRESOLVED),
    confidenceTier: EDGE_CONFIDENCE_TIERS.UNRESOLVED,
    resolved: false,
    specifier: record.specifier,
    reason: ambiguityReason,
    resolutionStatus: "AMBIGUOUS",
    candidates: candidatePaths,
    provider: MODULE_PROVIDER.id,
    factProvider: MODULE_PROVIDER,
    evidence: evidence(file, record.line),
  };
  generation.edges.push(edge);
  return edge;
}

function addModuleEvidence(generation, files, root, selectedFiles = files) {
  const fileByPath = new Map(files.map((file) => [normalizePath(file.path), file]));
  let resolved = 0;
  let unresolved = 0;
  let ambiguous = 0;
  for (const file of selectedFiles) {
    const outcomes = moduleRecords(file).map((record) => ({ record, result: resolveModule(record, file, root) }));
    const protectedTargets = new Set(outcomes
      .filter(({ result }) => result.status !== "AMBIGUOUS" && result.resolved && inside(root, result.resolved))
      .map(({ result }) => `file:${normalizePath(relative(root, result.resolved))}`));
    for (const { record, result } of outcomes) {
      const targetPath = result.resolved && inside(root, result.resolved) ? normalizePath(relative(root, result.resolved)) : null;
      const targetFile = targetPath ? fileByPath.get(targetPath) : null;
      const candidatePaths = moduleCandidatePaths(result, root);
      const existing = result.status === "AMBIGUOUS"
        ? abstainFromAmbiguousImport(generation, file, record, candidatePaths, result.reason, protectedTargets)
        : generation.edges.find((edge) => edge.kind === "IMPORTS"
          && edge.source === `file:${file.path}`
          && (targetFile ? edge.target === `file:${targetPath}` : edge.target === null && edge.specifier === record.specifier));
      const status = result.status === "AMBIGUOUS"
        ? "AMBIGUOUS"
        : targetFile ? "RESOLVED" : "UNRESOLVED";
      const claim = {
        provider: MODULE_PROVIDER.id,
        version: MODULE_PROVIDER.version,
        language: record.language,
        specifier: record.specifier,
        status,
        reason: targetFile ? result.reason : result.reason ?? "resolved_target_not_in_generation",
        candidates: candidatePaths,
        resolutionTier: result.resolutionTier ?? null,
        evidence: evidence(file, record.line),
      };
      if (existing) {
        existing.providerResolutions = [...(existing.providerResolutions ?? []), claim];
      } else if (targetFile) {
        const target = targetFile ? `file:${targetPath}` : null;
        const tier = target ? EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION : EDGE_CONFIDENCE_TIERS.UNRESOLVED;
        generation.edges.push({
          id: `edge:IMPORTS:file:${file.path}->${target ?? `unresolved:${record.specifier}`}:${MODULE_PROVIDER.id}:${record.line}`,
          kind: "IMPORTS",
          source: `file:${file.path}`,
          target,
          confidence: tierConfidence(tier),
          confidenceTier: tier,
          resolved: Boolean(target),
          specifier: target ? null : record.specifier,
          reason: target ? null : claim.reason,
          provider: MODULE_PROVIDER.id,
          factProvider: MODULE_PROVIDER,
          providerResolutions: [claim],
          evidence: evidence(file, record.line),
        });
      }
      if (status === "RESOLVED") resolved += 1;
      else if (status === "AMBIGUOUS") ambiguous += 1;
      else unresolved += 1;
    }
  }
  return { provider: MODULE_PROVIDER.id, resolved, unresolved, ambiguous };
}

function addDomainFact(generation, provider, file, category, name, line, edgeKind, attributes = {}) {
  const node = providerNode(provider, file, category, name, line, attributes);
  generation.nodes.push(node);
  generation.edges.push(providerEdge(provider, file, edgeKind, node.id, line, attributes));
  return node;
}

function addFrameworkEvidence(generation, files) {
  const summary = { provider: FRAMEWORK_PROVIDER.id, routes: 0, events: 0, database: 0, deployment: 0, gatedFiles: 0 };
  for (const file of files) {
    const imports = importedPackages(file.text);
    const stacks = ["next-express", "fastapi-django", "tauri-axum"].filter((stack) => frameworkGateActive(imports, stack));
    const eventGate = domainGateActive(imports, "event");
    const databaseGate = domainGateActive(imports, "database");
    const deploymentGate = /(^|\/)(?:\.github\/workflows|deploy|deployment|infrastructure)(\/|$)/i.test(file.path)
      || /(?:^|\/)Dockerfile$/i.test(file.path);
    if (stacks.length || eventGate || databaseGate || deploymentGate) summary.gatedFiles += 1;
    for (const stack of stacks) {
      for (const fact of extractRoutes({ stack, text: file.text, path: file.path })) {
        const name = fact.kind === "route" ? `${fact.method} ${fact.path}` : fact.name;
        addDomainFact(generation, FRAMEWORK_PROVIDER, file, fact.kind === "route" ? "HttpRoute" : "HttpHandler", name, fact.line, fact.kind === "route" ? "ROUTES_TO" : "CONTAINS", {
          stack,
          method: fact.method ?? null,
          routePath: fact.path ?? null,
          handler: fact.handler ?? null,
          confidenceTier: EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC,
          confidence: tierConfidence(EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC),
        });
        summary.routes += fact.kind === "route" ? 1 : 0;
      }
    }
    if (eventGate) {
      for (const fact of extractEventFacts({ text: file.text, path: file.path })) {
        addDomainFact(generation, FRAMEWORK_PROVIDER, file, "EventTopic", fact.topic, fact.line, fact.edge, {
          frameworkDomain: "event", role: fact.kind,
          confidenceTier: EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC,
          confidence: tierConfidence(EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC),
        });
        summary.events += 1;
      }
    }
    if (databaseGate) {
      for (const fact of extractDatabaseFacts({ text: file.text, path: file.path })) {
        if (!fact.name || !fact.edge) continue;
        addDomainFact(generation, FRAMEWORK_PROVIDER, file, "DatabaseModel", fact.name, fact.line, fact.edge, {
          frameworkDomain: "database", role: fact.kind,
          confidenceTier: EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC,
          confidence: tierConfidence(EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC),
        });
        summary.database += 1;
      }
    }
    if (deploymentGate) {
      for (const fact of extractDeploymentFacts({ text: file.text, path: file.path })) {
        const name = fact.action ?? fact.type;
        if (!name) continue;
        addDomainFact(generation, FRAMEWORK_PROVIDER, file, "Deployment", name, fact.line, fact.edge, {
          frameworkDomain: "deployment", role: fact.kind,
          confidenceTier: EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC,
          confidence: tierConfidence(EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC),
        });
        summary.deployment += 1;
      }
    }
  }
  return summary;
}

function addSchemaAndIacEvidence(generation, files) {
  const sql = { provider: SQL_PROVIDER.id, facts: 0 };
  const terraform = { provider: TERRAFORM_PROVIDER.id, facts: 0 };
  for (const file of files) {
    const profile = profileForPath(file.path);
    const sqlFacts = profile === "sql" || profile === "migration"
      ? extractSqlFacts(file.text)
      : profile === "dockerfile" ? extractDockerfileFacts(file.text) : [];
    for (const fact of sqlFacts) {
      const name = fact.name ?? `${fact.kind}:${fact.line}`;
      addDomainFact(generation, SQL_PROVIDER, file, `Sql${fact.kind[0].toUpperCase()}${fact.kind.slice(1)}`, name, fact.line, "CONFIGURES", { profile, table: fact.table ?? null });
      sql.facts += 1;
    }
    if (profile !== "terraform") continue;
    for (const fact of extractTerraformFacts(file.text)) {
      const name = `${fact.type}.${fact.name}`;
      addDomainFact(generation, TERRAFORM_PROVIDER, file, "TerraformResource", name, fact.line, "CONFIGURES", { resourceType: fact.type, resourceName: fact.name });
      terraform.facts += 1;
    }
  }
  return { sql, terraform };
}

function addScipEvidence(generation, files, root, options, selectedPaths = null, allowExternalTargets = false) {
  if (options.scip === false || process.env.BLUEPRINT_SCIP === "0") return { provider: pythonScipProvider.id, state: "disabled", nodes: 0, edges: 0 };
  const semantic = collectSemanticEvidenceSync(
    { repoRoot: root, scipIndexPath: options.scipIndexPath },
    { providers: [pythonScipProvider] },
  );
  const lane = semantic.results[0] ?? null;
  const collected = lane?.output ?? { nodes: [], edges: [], reports: [], index: { state: "unavailable" } };
  const fileByPath = new Map(files.map((file) => [normalizePath(file.path), file]));
  const admittedNodeIds = new Set(generation.nodes.map((node) => node.id));
  let nodesAdded = 0;
  let edgesAdded = 0;
  for (const node of collected.nodes ?? []) {
    const file = fileByPath.get(normalizePath(node.path));
    if (selectedPaths && !selectedPaths.has(normalizePath(node.path))) continue;
    if (!file || node.kind === "file" || admittedNodeIds.has(node.id)) continue;
    node.evidence = (node.evidence ?? []).map((item) => ({ ...item, contentHash: file.contentHash ?? null }));
    node.factProvider = { id: pythonScipProvider.id, version: pythonScipProvider.version };
    generation.nodes.push(node);
    admittedNodeIds.add(node.id);
    nodesAdded += 1;
  }
  for (const edge of collected.edges ?? []) {
    const file = fileByPath.get(normalizePath(edge.evidence?.[0]?.path));
    if (selectedPaths && !selectedPaths.has(normalizePath(edge.evidence?.[0]?.path))) continue;
    if (!file || !admittedNodeIds.has(edge.source) || (!allowExternalTargets && edge.target && !admittedNodeIds.has(edge.target))) continue;
    edge.evidence = edge.evidence.map((item) => ({ ...item, contentHash: file.contentHash ?? null }));
    edge.factProvider = { id: pythonScipProvider.id, version: pythonScipProvider.version };
    generation.edges.push(edge);
    edgesAdded += 1;
  }
  return {
    provider: pythonScipProvider.id,
    state: collected.index?.state ?? "unavailable",
    nodes: nodesAdded,
    edges: edgesAdded,
    reports: collected.reports ?? [],
    disposition: lane?.disposition ?? null,
  };
}

function addBridgeEvidence(generation, files) {
  const collected = bridgeSeamProvider.collect({ files });
  generation.nodes.push(...collected.nodes);
  generation.edges.push(...collected.edges);
  return collected.summary;
}

export function augmentGenerationWithFirstPartyProviders(generation, repoRoot, files, options = {}) {
  const root = resolve(repoRoot);
  const summaries = {
    ingestion: auditSourceDispositions(root, files),
    modules: addModuleEvidence(generation, files, root),
    frameworks: addFrameworkEvidence(generation, files),
    ...addSchemaAndIacEvidence(generation, files),
    scip: addScipEvidence(generation, files, root, options),
    bridges: addBridgeEvidence(generation, files),
  };
  summaries.structuralIntelligence = augmentStructuralIntelligence(generation, files);
  summaries.frameworkIntelligence = augmentFrameworkIntelligence(generation, files);
  summaries.portableIdentity = attachPortableIdentities(generation);
  summaries.conventions = detectProjectConventions(files);
  const layers = [
    { id: MODULE_PROVIDER.id, version: MODULE_PROVIDER.version, role: "supplemental", precisionTier: "EXACT_OR_TYPED_UNRESOLVED" },
    { id: FRAMEWORK_PROVIDER.id, version: FRAMEWORK_PROVIDER.version, role: "supplemental", precisionTier: "EVIDENCE_BOUND_HEURISTIC" },
    { id: SQL_PROVIDER.id, version: SQL_PROVIDER.version, role: "supplemental", precisionTier: "EXACT_SYNTAX" },
    { id: TERRAFORM_PROVIDER.id, version: TERRAFORM_PROVIDER.version, role: "supplemental", precisionTier: "EXACT_SYNTAX" },
    { id: pythonScipProvider.id, version: pythonScipProvider.version, role: "supplemental", precisionTier: "COMPILER", state: summaries.scip.state },
    { id: bridgeSeamProvider.id, version: bridgeSeamProvider.version, role: "supplemental", precisionTier: "EXACT_SYNTAX" },
    { id: STRUCTURAL_INTELLIGENCE_PROVIDER.id, version: STRUCTURAL_INTELLIGENCE_PROVIDER.version, role: "supplemental", precisionTier: "EXACT_OR_TYPED_UNRESOLVED" },
    { id: FRAMEWORK_INTELLIGENCE_PROVIDER.id, version: FRAMEWORK_INTELLIGENCE_PROVIDER.version, role: "supplemental", precisionTier: "EVIDENCE_BOUND" },
  ];
  return { schemaVersion: 1, summaries, layers };
}

export function augmentFileFactsWithFirstPartyProviders(generation, repoRoot, file, files, options = {}) {
  const root = resolve(repoRoot);
  const selected = [file];
  addModuleEvidence(generation, files, root, selected);
  addFrameworkEvidence(generation, selected);
  addSchemaAndIacEvidence(generation, selected);
  addScipEvidence(generation, files, root, options, new Set([normalizePath(file.path)]), true);
  addBridgeEvidence(generation, selected);
  augmentStructuralIntelligence(generation, selected);
  augmentFrameworkIntelligence(generation, selected);
  attachPortableIdentities(generation);
  return generation;
}
