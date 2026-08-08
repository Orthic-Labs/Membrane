const text = value => String(value ?? 'unknown');
const esc = value => text(value).replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]));

// Projection only: this module does not fetch, stage, activate, or roll back releases.
export function releaseChannelViewModel(snapshot) {
  const data = snapshot?.result?.data?.releaseChannel || snapshot?.releaseChannel || snapshot?.data || snapshot || {};
  const support = data.support || {};
  return {
    channel: text(data.channel), release: text(data.release),
    support: text(support.state), supportStartsAt: text(support.startsAt),
    supportEndsAt: support.endsAt == null ? 'open' : text(support.endsAt),
    schemaCompatibility: text(data.schemaCompatibility),
    migration: text(data.migration), rollback: text(data.rollback),
    update: data.signedUpdateEvidence ? 'available' : 'unavailable — missing signed update evidence',
    requiredUpdate: data.requiredUpdate || 'unknown — no authoritative update decision',
    updateEvidence: data.signedUpdateEvidence || null,
  };
}

export const projectReleaseChannel = releaseChannelViewModel;

export function renderReleaseChannel(snapshot, root) {
  const vm = releaseChannelViewModel(snapshot);
  root.innerHTML = `<section aria-label="Release channel"><h2>Release channel</h2><dl><dt>Channel</dt><dd>${esc(vm.channel)}</dd><dt>Release</dt><dd>${esc(vm.release)}</dd><dt>Support</dt><dd>${esc(vm.support)} (${esc(vm.supportStartsAt)} → ${esc(vm.supportEndsAt)})</dd><dt>Schema</dt><dd>${esc(vm.schemaCompatibility)}</dd><dt>Update</dt><dd>${esc(vm.update)}</dd><dt>Required update</dt><dd>${esc(vm.requiredUpdate)}</dd><dt>Migration</dt><dd>${esc(vm.migration)}</dd><dt>Rollback</dt><dd>${esc(vm.rollback)}</dd></dl></section>`;
}
