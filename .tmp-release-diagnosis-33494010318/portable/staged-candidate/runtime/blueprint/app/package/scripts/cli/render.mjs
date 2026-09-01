// D05: human-readable rendering for CLI output. Machine output (--json) goes
// to stdout as a stable schema envelope; diagnostics go to stderr.

export function machineError(code, message, details = {}) {
  return {
    schemaVersion: 1,
    error: { code, message, ...details },
  };
}

export function printResult(value, args = {}, { stderr = false } = {}) {
  const text = JSON.stringify(value, null, 2);
  if (stderr) process.stderr.write(`${text}\n`);
  else process.stdout.write(`${text}\n`);
}

export function renderStatus(payload) {
  const state = payload.state ?? "unknown";
  const generation = payload.manifest?.generationId ?? payload.generationId ?? "none";
  return `graph ${state} provider=${payload.provider ?? "?"} generation=${generation}`;
}

export function renderSearch(payload) {
  const lines = [`search "${payload.query ?? ""}" (generation ${payload.generationId ?? "?"}):`];
  for (const result of payload.results ?? []) {
    const where = result.path ? `${result.path}${result.span ? `:${result.span.startLine ?? "?"}` : ""}` : result.id;
    lines.push(`  ${result.rank ?? "?"}. ${result.name ?? result.id} — ${where}`);
  }
  return lines.join("\n");
}

export function renderExpand(payload) {
  const nodes = payload.nodes ?? payload.neurons ?? [];
  const edges = payload.edges ?? payload.synapses ?? [];
  return `expand: ${nodes.length} nodes, ${edges.length} edges (generation ${payload.generationId ?? "?"})`;
}

export function renderImpact(payload) {
  const edges = payload.edges ?? payload.affected ?? [];
  return `impact: ${edges.length} affected edges (generation ${payload.generationId ?? "?"})`;
}

export function renderArchitecture(payload) {
  const nodes = payload.nodes ?? payload.components ?? [];
  const edges = payload.edges ?? [];
  return `architecture: ${nodes.length} nodes, ${edges.length} edges (generation ${payload.generationId ?? "?"})`;
}

export function renderDocTruth(payload) {
  return `doc-truth: ${payload.claims?.length ?? 0} claims (generation ${payload.generationId ?? "?"})`;
}
