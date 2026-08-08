const esc = value => String(value ?? 'unknown').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const arr = (value, limit) => Array.isArray(value) ? value.slice(0, limit) : [];

// Hub renders evidence, not liveness. A process flag is intentionally ignored.
export function sourcesViewModel(snapshot) {
  const data = snapshot?.result?.data?.sourcesExplorer || snapshot?.result?.data?.sources || snapshot?.result?.data || snapshot?.data || snapshot?.payload || snapshot || {};
  const readiness = data.readiness || {};
  return {
    schemaVersion: data.schemaVersion || data.schema || 'sources-explorer.v1',
    repository: data.repository || {}, generation: data.generation || {}, clocks: data.clocks || {},
    readiness: { state: readiness.state || 'unknown', reason: readiness.reason || 'unknown — no authoritative evidence', authoritative: readiness.authoritative === true },
    parser: data.parser || {}, contribution: data.recentContribution || data.contribution || {},
    paths: arr(data.paths, 64), neighborhoods: arr(data.neighborhoods, 32),
  };
}

export function renderSourcesExplorer(snapshot, root) {
  const vm = sourcesViewModel(snapshot);
  const item = (value, fallback = 'unknown') => esc(value || fallback);
  const paths = vm.paths.length ? vm.paths.map(p => `<li><code>${item(p.path || p.sourceRef)}</code> · ${item(p.kind)} · ${item(p.evidence)}</li>`).join('') : '<li>unknown — no bounded paths</li>';
  const neighborhoods = vm.neighborhoods.length ? vm.neighborhoods.map(n => `<li><code>${item(n.path)}</code> ${item(n.relation)}: ${arr(n.neighbors, 64).map(esc).join(', ') || 'unknown'}</li>`).join('') : '<li>unknown — no bounded neighborhoods</li>';
  root.innerHTML = `<section class="sources-explorer" aria-label="Sources explorer"><header><h2>Sources explorer</h2><small>read-only evidence · schema: ${item(vm.schemaVersion)}</small></header><dl><dt>Repository</dt><dd>${item(vm.repository.id)} · ${item(vm.repository.origin)} · ${item(vm.repository.revision)}</dd><dt>Generation</dt><dd>${item(vm.generation.id)} · ${item(vm.generation.release)} · ${item(vm.generation.index)}</dd><dt>Readiness</dt><dd>${item(vm.readiness.state)} — ${item(vm.readiness.reason)}${vm.readiness.authoritative ? ' · authoritative' : ' · authority unavailable'}</dd><dt>Parser</dt><dd>${item(vm.parser.name)} ${item(vm.parser.version)} · ${arr(vm.parser.languages, 64).map(esc).join(', ') || 'unknown'}</dd><dt>Recent contribution</dt><dd>${item(vm.contribution.summary)} · ${arr(vm.contribution.paths, 64).map(p => `<code>${esc(p)}</code>`).join(', ') || 'unknown'}</dd></dl><h3>Bounded paths</h3><ul>${paths}</ul><h3>2D neighborhoods</h3><ul>${neighborhoods}</ul><small>Observed clocks: ${item(vm.clocks.observedAtUnixMs)} · contribution: ${item(vm.clocks.contributionAtUnixMs)}</small></section>`;
}
