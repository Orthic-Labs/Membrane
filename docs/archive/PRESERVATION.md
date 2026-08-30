# Documentation preservation map

Source revision: `d84322c3df182ff1d6ef7ca96fe94aea22273894`.

Every documentation path deleted by current topology migration has one terminal disposition. At reconciliation time, 127 of 150 deleted `docs/**` paths had an exact Git-blob match under `docs/archive/**`. Remaining 23 paths are mapped below. `exact relocation` means source & target Git blob IDs match. `superseded` means target intentionally changed; source blob remains addressable at source revision & is not current authority.

| Source path | Source blob | Successor/disposition |
|---|---|---|
| `docs/design/MEMBRANE-CURRENT-STATE-MANIFEST.json` | `5cca1f3a868b3aebf5956abb841248a49e4f6a75` | Superseded by `docs/current/architecture/current-state-manifest.json`. |
| `docs/design/THREAT-MODEL-MCP-V1.md` | `86d0dd9c67bc67310e066b575f6770cf1a3d1e9c` | Superseded by `docs/current/architecture/security/mcp-threat-model.md`. |
| `docs/design/agent-findings-lane.md` | `6cd97736958f9b90acf981591ad2001a2562b09b` | Reclassified & revised as pending support spec `docs/pending/capabilities/blueprint/findings-lane.md`. |
| `docs/design/hub-redesign/DECISION-PROCESS-ARCHITECTURE.md` | `ecbe3bb6ff2b22d527e7d934121e7e1aa6938edd` | Superseded by `docs/current/architecture/adr/tray-daemon-process.md`. |
| `docs/design/hub-redesign/MEMBRANE-BRAND-IDENTITY.md` | `1dcdbd5c7ffdb165b5f618a054fbce5c16902fef` | Exact relocation to `docs/pending/design/membrane-brand-identity.md`. |
| `docs/design/hub-redesign/TRAY-DAEMON-SPEC.md` | `9ab60cb232a02e2caf5e244851fc3008b4ab0562` | Superseded by `docs/current/architecture/runtime/tray-daemon-contract.md`. |
| `docs/design/hub-redesign/dashboard.html` | `29d2b54d3ac03041927966c0d848c8287c1a1c45` | Retired visual prototype; no current-product successor. Source remains addressable at source revision. |
| `docs/design/hub-redesign/fonts/SplineSansMono-400.woff2` | `14581f46fafc75ce726e9f1e5075f834d21e7069` | Exact relocation to `docs/pending/design/hub/fonts/SplineSansMono.woff2`. |
| `docs/design/hub-redesign/fonts/SplineSansMono-500.woff2` | `14581f46fafc75ce726e9f1e5075f834d21e7069` | Exact relocation to `docs/pending/design/hub/fonts/SplineSansMono.woff2`; source weights were byte-identical. |
| `docs/design/hub-redesign/fonts/Tanker-400.woff2` | `f4003172188f7ae71081205e681822e1e213072d` | Exact relocation to `docs/pending/design/hub/fonts/Tanker-400.woff2`. |
| `docs/design/hub-redesign/hub-mockup.html` | `80a05b094d348e4aabe23cb660266fae2b9dece8` | Revised pending visual reference at `docs/pending/design/hub/hub-mockup.html`. |
| `docs/design/membrane-live-diagnostics-final-architecture.md` | `8e9af2f0fdfdb2acd2fe57b19ea586dc3ad49054` | Superseded by `docs/current/architecture/live-diagnostics.md`. |
| `docs/design/update-dual-signature.md` | `7d65794dfeaea1a8f39a74e4c6100cceabee7d48` | Exact relocation to `docs/current/architecture/security/update-admission.md`. |
| `docs/operations/resident-lifecycle.md` | `dff0c034331bc30a7d86f4d5e9d70499a3d420a4` | Superseded by `docs/current/architecture/adr/tray-daemon-process.md` & `docs/current/architecture/runtime/tray-daemon-contract.md`; historical operator copy remains at `docs/archive/superseded/operations/resident-lifecycle.md`. |
| `docs/pending/ADAPT-HARNESS-EFFICIENCY-INSIGHTS.md` | `82f9dd9acdf0e0a034f519da68806de1c803ed56` | Revised pending support spec at `docs/pending/capabilities/adapt/harness-efficiency.md`. |
| `docs/pending/MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md` | `260f59cf5a82d786d8f421f9898b6a6fb6bf915c` | Revised optional experiment at `docs/pending/experiments/semantic-context-advisor.md`. |
| `docs/plans/membrane-live-diagnostics-final-architecture-revised.md` | `8e9af2f0fdfdb2acd2fe57b19ea586dc3ad49054` | Duplicate of deleted Live Diagnostics design; superseded by `docs/current/architecture/live-diagnostics.md`. |
| `docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md` | `cedac3d23068ca01c2cea78998d07f7347976859` | Superseded by `docs/current/architecture/subsystems/adapt.md`. |
| `docs/subsystems/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md` | `31e6aca77d6e8f4bc9ec53b8d1db7b1fb5215366` | Superseded by `docs/current/architecture/subsystems/blueprint.md`. |
| `docs/subsystems/CODERIGHT-MEMBRANE-OBSERVABILITY-LEARNING-AND-EVAL-INTEGRATION.md` | `a7f928c6b953d0c651696462ac56f678dbdb4677` | Superseded by `docs/current/architecture/integrations/coderight.md`. |
| `docs/subsystems/LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md` | `0aa58c87e8f3e5e108c4891e90c0189b7ab7dfa0` | Superseded by `docs/current/architecture/subsystems/ledger.md`. |
| `docs/subsystems/MEMBRANE-CROSS-SUBSYSTEM-IMPROVEMENTS-AND-EVIDENCE-GATES.md` | `dc2fd12d757e96c16b79e56b6cc28115fe0029f1` | Superseded by `docs/current/architecture/cross-subsystem-evidence.md`. |
| `docs/subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md` | `b7a2af2a12a5e6a7788aaf948e37dd56bd4c9121` | Superseded by `docs/current/architecture/membrane.md`. |

Exact archive matches are verified by comparing each deleted source blob with every file under `docs/archive/**`; filename similarity is not accepted as preservation evidence.
