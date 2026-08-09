# Roadmap

Cortex 1.0 ships the local evidence-backed repository index, plus the
qualification, hardening, security, and release-policy gates that make a
real release safe to ship. What follows is the explicit measured roadmap
**after** 1.0: deferred surfaces, support policy, and the milestones each
would need to clear to re-open.

## 1.0 — shipped

| Capability | Packet | Status |
|---|---|---|
| N-2 store migration + repair | D50 | ✅ |
| Performance envelopes (small / medium / large) | D50 | ✅ |
| Cross-platform path compatibility (macOS, Linux, Windows) | D50 | ✅ |
| Hostile-repository security suite | D51 | ✅ |
| Plugin trust boundary (no escalation) | D51 | ✅ |
| Deterministic soak + fault injection | D52 | ✅ |
| Immutable release gate chain (all tests, qualification, package, signing, SBOM, provenance, clean-host) | D53 | ✅ |
| OIDC trusted publishing (no long-lived NPM_TOKEN) | D53 | ✅ |

## Deferred — measured, with reversal conditions

| Surface | Decision | Why deferred |
|---|---|---|
| Node SEA single executable | [001-node-sea.md](../release/decisions/001-node-sea.md) | Runtime bundle already meets the contract; SEA useful for sandboxed/locked-down environments, not 1.0 |
| Public Rust crate | [002-rust-crate.md](../release/decisions/002-rust-crate.md) | Node SDK already serves the public surface; rewrite would be a separate product, not a release surface |
| Hosted remote / team mode | [003-remote-team-mode.md](../release/decisions/003-remote-team-mode.md) | Federation contracts exist; hosted service needs an explicit data-handling contract and a separate release line |
| Third-party plugin marketplace | [004-plugin-marketplace.md](../release/decisions/004-plugin-marketplace.md) | Contracts and trust boundary are ready; review process + registry are the missing operational pieces |

## Support policy

- **Current line:** 0.2.x (LTS) — security fixes only.
- **Next line:** 1.0.0 — feature additions, qualification-gated, immutable
  releases only.
- **Backports to 0.1.x:** None. Customers on 0.1.x must upgrade to a
  supported line.
- **Compatibility window:** the schema version of every public store is
  recorded in `release/compatibility.json`; consumers can pin and detect.

## What does NOT change in 1.x

- **The CLI surface.** D05 makes the CLI a thin adapter; the canonical
  surface is the shared application service. New commands add capabilities;
  the names of existing commands stay stable.
- **The MCP six-tool surface.** D07 binds the server root at startup;
  adding a new tool is a typed proposal, not a silent widening.
- **The language depth matrix.** Tier A/B/C labels (see
  `release/compatibility.json` `languageDepth`) are the public claim, not
  the raw grammar count.
- **The do-not-absorb list.** "Do not turn Cortex into general user memory
  or final cross-layer context admission" (Membrane/Crypt boundary) and the
  other eight items in §8 of the runbook remain stable through 1.x.

## How to contribute

See `CONTRIBUTING.md` and the runbook's packet format. Every change ships
through `qualification.yml`; a failing qualification gate blocks merge.
