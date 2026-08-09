// D34: cross-stack providers — queue/event producer-topic-consumer,
// generated client/handler contracts, DB/ORM model-table-migration-query,
// and deployment relationships. Import/config/schema gated; name-only
// matches remain unresolved or heuristic.

import { extractRoutes, frameworkGateActive } from "./http/index.mjs";

export const CROSS_STACK_EDGES = Object.freeze(["PRODUCES", "CONSUMES", "GENERATES", "READS", "WRITES", "CONFIGURES", "DEPLOYS"]);

export function extractEventFacts({ text, path }) {
  const facts = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    const produce = line.match(/(?:publish|produce|emit)\(\s*["'`]([A-Za-z0-9_.-]+)["'`]/i);
    if (produce) facts.push({ kind: "producer", topic: produce[1], line: i + 1, edge: "PRODUCES", confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
    const consume = line.match(/(?:subscribe|consume|onMessage|handler)\(\s*["'`]([A-Za-z0-9_.-]+)["'`]/i);
    if (consume) facts.push({ kind: "consumer", topic: consume[1], line: i + 1, edge: "CONSUMES", confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
  }
  return facts;
}

export function extractDatabaseFacts({ text, path }) {
  const facts = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    const model = line.match(/(?:model|table)\s+([A-Za-z0-9_]+)/i);
    if (model) facts.push({ kind: "model", name: model[1], line: i + 1, confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
    const read = line.match(/\.(find|get|query|select)\(/i);
    if (read) facts.push({ kind: "read", line: i + 1, edge: "READS", confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
    const write = line.match(/\.(insert|create|update|delete|save)\(/i);
    if (write) facts.push({ kind: "write", line: i + 1, edge: "WRITES", confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
  }
  return facts;
}

export function extractDeploymentFacts({ text, path }) {
  const facts = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    const deploy = line.match(/^(\s*-\s*)?(?:uses|action|step|deploy)\s*:\s*([A-Za-z0-9_.-]+)/i);
    if (deploy) facts.push({ kind: "deploy", action: deploy[2], line: i + 1, edge: "DEPLOYS", confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
    const tf = line.match(/^\s*resource\s+"([^"]+)"/);
    if (tf) facts.push({ kind: "resource", type: tf[1], line: i + 1, edge: "CONFIGURES", confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
  }
  return facts;
}

export { extractRoutes, frameworkGateActive };
