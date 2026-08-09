# Threat model

This document is the authoritative mapping from hostile input classes to the
controls that neutralise them. It exists because "be safe" is not a gate; the
table below is. The qualification job in
`.github/workflows/qualification.yml` (and the `tests/security/**` suite it
runs) is what turns each row into a release gate.

## Hostile input classes

| # | Class | Fixture | Control | Gate |
|---|---|---|---|---|
| 1 | Oversized file | `huge/generated.js` (single-line 64KB) | read/lines budget drops it from the graph; bounded output asserted | `hostile-indexing.test.mjs` |
| 2 | Deep tree | `a/.../z/deep.ts` (26 levels) | traversal is bounded; output stays small | `hostile-indexing.test.mjs` |
| 3 | Symlink escape | `escape -> /tmp` | scanner never follows symlinked dirs; `canonicalRoot` realpaths the root | `hostile-indexing.test.mjs`, `cross-platform-paths.test.mjs` |
| 4 | Path case tricks | `SRC/Config.ts` vs `src/Config.ts` | case-insensitive FS collapses to one identity; case-sensitive FS keeps them distinct | `cross-platform-paths.test.mjs` |
| 5 | Malformed store | `.agent/graph/graph.db` of 0xdb bytes | `graphStatus` returns `corrupt`; build removes and recreates; no uncaught crash | `hostile-indexing.test.mjs`, `static-provider.mjs` `writeGeneration` |
| 6 | Event overflow | 300-file churn burst | journal drains in bounded passes; overflow surfaces as `event_overflow` degradation | `hostile-indexing.test.mjs` |
| 7 | Prompt injection in docs/comments | AGENTS.md with `curl | sh` instructions | repo docs are DATA; no execution during indexing; secret egress asserted | `secret-egress.test.mjs` |
| 8 | Secret patterns | `deploy/*.env`, `service-account.json` | `redactForEgress` + support-bundle allowlist; known-secret corpus never appears on any surface | `secret-egress.test.mjs`, `support-bundle-redaction.test.mjs` |
| 9 | Poisoned grammar manifest | `grammars/*.toml` with `../../../etc/passwd` + bogus hash | vendor-dir confinement + SHA-256 hash validation | `poisoned-manifests.test.mjs` |
| 10 | Poisoned plugin manifest | escalated `filesystem/network/process` | `definePlugin` rejects anything broader than `repo-read/none/none` | `plugin-boundary.test.mjs` |
| 11 | Archive traversal | `bundle/bomb.tar` | extraction refusals for `../` entries | `poisoned-manifests.test.mjs` |
| 12 | Unsigned / downgrade / replay updates | `lib/update/**` fixtures | signed-manifest verification, downgrade + replay rejection | `update-contract.test.mjs`, `update-rollback.test.mjs` |

## Assertions that hold for every row

- **No repository content becomes instruction.** Indexing never executes a
  repository-provided command; `proof-of-exec.txt` markers are asserted absent.
- **No secret exits.** CLI, MCP, UI, support bundle, doctor, and logs are
  scanned for the full known-secret corpus.
- **No path escapes root.** Root-relative outputs, symlink resolution, and
  separator normalisation are asserted in the path suite and the hostile tree.
- **No unbounded output or loop.** Every test bounds combined stdout+stderr and
  requires a normal process exit (`0..3`), never a signal or uncaught crash.
- **Typed degradation over silent corruption.** `corrupt`, `missing`, `stale`
  and `event_overflow` are first-class states; an untyped crash is the failure.

## Build-time providers

Compiler/LSP/build providers are opt-in (D28): nothing spawns them during a
default index, they declare network/process permissions, and the qualification
network-boundary check proves there are zero undeclared network-capable
operations in the shipped surface.
