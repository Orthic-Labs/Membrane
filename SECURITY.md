# Security Policy

Cortex is a local, evidence-backed repository index. Its threat model assumes
an **untrusted repository tree** standing next to the trusted CLI, service, and
SDK: hostile files, docs, stores, and manifests must never execute, exfiltrate,
or escalate.

## Supported versions

| Version | Status |
|---|---|
| 0.2.x (current line) | Supported |
| 1.0.0 release candidates | Supported once the 1.0 gate (D50–D53) passes |
| Older | Non-negotiable N-2 store migration, no feature support |

## Reporting a vulnerability

Report privately to the Orthic Labs maintainers. Do not open a public issue
for a security defect. Every report is triaged under the qualification
mapping in `docs/reference/threat-model.md`.

## Trust boundaries

1. **Repository content is data.** No file in an indexed repo becomes an
   instruction: AGENTS.md, comments, and generated docs are extracted as facts
   with provenance, never executed, never appended to a prompt as authority.
2. **Root confinement.** The scanner, watcher, store, MCP server, and SDK all
   operate inside the enrolled repository root. Symlink escape, `..` traversal
   (including in archives), case tricks, and Windows UNC/junction paths cannot
   change repository identity or escape the scope.
3. **No repository-command execution during indexing.** Compiler/LSP/build
   providers are opt-in and declare network/process permissions. Nothing in the
   default pipeline spawns a command named by repository content.
4. **No secret egress.** A known-secret corpus
   (`fixtures/security/secret-corpus.json`) is asserted never to appear on any
   egress surface: CLI stdout/stderr, MCP tool output, UI payloads, support
   bundles, doctor output, or logs. Secrets stay in the repository.
5. **Plugin/grammar/update trust.** Plugin manifests may not escalate
   permissions (`repo-read`, `network: none`, `process: none`); grammar
   manifests may not point outside the vendor dir or ship an invalid hash;
   update manifests are signed and reject unsigned, downgrade, and replay.
6. **Bounded work.** Oversized files, deep trees, event overflow, and malformed
   stores produce typed degradation (e.g. `corrupt`), never unbounded output,
   loops, or uncaught crashes.

## Qualification mapping

Every control above maps to an automated gate in
`.github/workflows/qualification.yml`:

| Control | Gate |
|---|---|
| Repository content is data | `tests/security/secret-egress.test.mjs`, `tests/security/poisoned-manifests.test.mjs` |
| Root confinement | `tests/security/hostile-indexing.test.mjs`, `tests/mcp-root-confinement.test.mjs` |
| No repository-command execution | `tests/security/secret-egress.test.mjs` |
| No secret egress | `tests/security/secret-egress.test.mjs`, `tests/support-bundle-redaction.test.mjs` |
| Plugin/grammar/update trust | `tests/plugin-boundary.test.mjs`, `tests/security/poisoned-manifests.test.mjs`, `tests/update-contract.test.mjs` |
| Bounded work | `tests/security/hostile-indexing.test.mjs` |
| Network boundary | `node scripts/ci/check-network-boundary.mjs` |

A failing security gate blocks release exactly like a failing functional gate:
the 1.0 release cannot bypass any qualification job.
