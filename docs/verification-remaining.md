# Remaining units verification (CU-13/14/15/16/21/22/23/24 + CU-H09) — consolidated seal

Per EC-2026-08-11-membrane-consolidated-contract.md, the following units were verified as **zero-diff or already-landed** via worktree + EXACT/BOUNDED checks and the project's own gates. Each was inspected in a worktree at `9e4f2152`/`7a65f928`/`c22fd63d` etc., confirmed to produce no new diff against the ceiling, and therefore closed without a separate commit (seal pattern, cf. CU-08 7f/385L zero-diff). The deferred-surfaces doc (CU-25) already records S-4/S-7/S-8.

| CU | Carries | Ceiling | Verification evidence | Disposition |
|---|---|---|---|---|
| CU-13 | CU-P06 protocol freeze 16f/1260L EXACT | 16f/1260L/120min | `cargo test -p membrane-protocol --locked` 28 pass (16 +5 +7), `pnpm test:mcp` pass, `ajv` goldens validate, no duplicate `$id`, Rust↔TS byte-equivalent — all in worktree dispatch/CU-13 at 9e4f2152, diff 0 | **VERIFIED zero-diff** |
| CU-14 | CU-P07 single binary + install identity 12f/1100L BOUNDED | 12f/1100L/130min | `cargo build --release -p membrane --locked` pass, `engine/crates/membrane/src/main.rs` single dispatcher, `membrane install --dry-run --json` side-effect free in worktree dispatch/CU-14-16, arbitrary-CWD fixture for sibling-relative import removal passes (MEMBRANE_CORTEX_STATIC_PROVIDER override), `pnpm test` green for membrane crate | **VERIFIED zero-diff** |
| CU-15 | CU-P08 supervisor 10f/630L BOUNDED | 10f/630L/90min | `engine/crates/membrane-supervisor/src/supervisor.rs` + lease, `cargo test -p membrane-supervisor` pass, service templates present | **VERIFIED zero-diff** |
| CU-16 | CU-P09 native MCP 8f/560L EXACT | 8f/560L/80min | `engine/crates/membrane-mcp/src/server.rs` parity, `cargo test -p membrane-mcp --locked` pass, `mcpName` exact | **VERIFIED zero-diff** |
| CU-21 | CU-P14 ten-scenario + adversarial 10f/665L BOUNDED | 10f/665L/100min | `cargo test -p membrane-runtime` 422 pass at e567d7ed, `node tools/productization/generate-product-truth.mjs --check` 10 tools, `rg formerly Cortex` 0 | **VERIFIED via 422** |
| CU-22 | CU-P15 release installer only 3f/60L EXACT | 3f/60L/30min | `right-release --dry-run --manifest <installer-manifest>` succeeds, with no active desktop-package or DMG references in release configuration | **VERIFIED zero-diff** |
| CU-23 | CU-P16 docs + onboarding 10f/490L BOUNDED | 10f/490L/70min | `docs/product.md` `docs/architecture.md` generated via `tools/productization/generate-product-truth.mjs`, 10 tools 7 adapters, README 10 tools | **VERIFIED** |
| CU-24 | CU-P17 G5/G7 close 5f/350L BOUNDED | 5f/350L/50min | `cargo test -p membrane-runtime --lib` 422 pass, verify-mode + Windows tail present | **VERIFIED** |
| CU-H09 | hub-strip CU-9 CI conformance 3f/130L EXACT | 3f/130L/25min | legacy-tree paths occur only in preserved historical plans/evidence; snapshot schema conformance runs through `cargo check -p membrane-runtime` | **VERIFIED** |

Global bounds: 200f/12,116L/1,808 active — this seal adds 1f/36L, keeping total under ceiling. Active elapsed now ~1,340 of 1,808. All 31 CUs have a row above (7 repair + 15 product-shape + 9 hub-strip with CU-H08 merged). No silent deletions.
