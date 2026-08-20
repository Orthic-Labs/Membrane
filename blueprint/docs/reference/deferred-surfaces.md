# Deferred surfaces

Cortex 1.0 ships the local evidence-backed repository index. The surfaces
below are measured, documented, and **not** part of the 1.0 release seal.
Each has a decision record under `release/decisions/` and a reversal
condition. The compatibility claim (`release/compatibility.json`,
`release/compatibility.template.json`) references them; `docs/roadmap.md`
ships the same table to the public.

This file is the reference the release seal (`docs/operations/release.md`
§ Release seal) cites — it does not add a new mechanism, it records what
the seal intentionally excludes.

## Deferred — 4 surfaces (D53)

| Surface | Decision | Status | Reversal |
|---|---|---|---|
| Node SEA single executable | [001-node-sea.md](../../release/decisions/001-node-sea.md) | Deferred | User-facing feature requires single-executable, or runtime bundle impractical on a supported platform |
| Public Rust crate | [002-rust-crate.md](../../release/decisions/002-rust-crate.md) | Declined (not deferred) | Documented community demand for first-class Rust bindings that SDK cannot serve via FFI |
| Hosted remote / team mode | [003-remote-team-mode.md](../../release/decisions/003-remote-team-mode.md) | Deferred | Hosted data-handling contract + tenant boundary + prompt-injection policy specified; separate 1.x line |
| Third-party plugin marketplace | [004-plugin-marketplace.md](../../release/decisions/004-plugin-marketplace.md) | Deferred | Review process + registry contract shipped without widening plugin trust boundary |

Notes:

- **Node SEA** — 1.0 uses the portable runtime bundle (`scripts/release/stage-runtime.mjs`) plus signed `.pkg`/`.exe`. SEA measured 2026-08-04 at ~110 MB and adds platform-specific postinstall; not needed for 1.0's "no system Node required" contract (D14). Future use: sandboxed base images, per-user launchd/Service executable.
- **Rust crate** — core ships as Node ESM + `node:sqlite` + WASM grammars. A placeholder crate would invite low-quality bindings that take ownership from the typed SDK (`CortexClient`, `EmbeddedCortexClient`). A real crate would be a multi-quarter ABI reproduction — separate product, not a release surface (runbook do-not-absorb list).
- **Remote / team mode** — federation envelope (`graph/federation/`, D35) exists but is not enabled by default and not connected to a hosted service. 1.0 is local-only; repository content never leaves the machine. Self-hosted federation is supported; managed offering is deferred.
- **Plugin marketplace** — contracts (`sdk/providers.mjs`) and trust boundary (`permissions.filesystem/network/process` ≤ `repo-read/none/none`, D51) ship in 1.0, but curated catalog / automatic discovery does not. Licensing: independent plugins may use Apache-2.0 SDK/schemas/examples, must not copy core.

## What is not deferred (ships in 1.0)

- N-2 store migration + repair (D50), performance envelopes (D50), cross-platform paths (D50), hostile-repo security suite + plugin trust boundary (D51), soak + fault injection (D52), candidate/SBOM/checksum/provenance/clean-host contracts (D53), OIDC trusted publishing (D53) — see `docs/roadmap.md`.

## Release-seal relationship

The seal (`docs/operations/release.md` § Release seal, `.agent/dispatch/state.json` D53, `.github/workflows/release-candidate.yml`) is verified by `scripts/release/verify-release.mjs`. That gate asserts the candidate inventory, SBOM, checksums, and clean-host receipts, and asserts that no deleted in-repo signing workflow is cited — it does not resurrect deferred surfaces. Adding any of the four surfaces above requires a new decision record and a new packet; the seal will not pass without it.
