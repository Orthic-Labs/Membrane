# Plugin/MCP source & closure review — 2026-09-16

Verdict: **OPEN — partial implementation, zero newly lifecycle-closed atoms.** Reviewed dirty working tree based on `3715589ba1f7a15f5c650beed577d83b15e6e398`; this receipt does not qualify an installed release.

## Verified progress

- Root HTTP/bearer MCP declaration, Claude HTTP declaration & membrane-client hooks, host projection versions at 0.1.24, candidate client compilation/staging & release-version stamping are present.
- Activation contains host MCP registration, stable installed-root checks, identity/health checks & rollback/preservation helpers. This establishes partial wiring, not native plugin installation or real host delivery.
- `node scripts/ci/check-release-identity.mjs` passed in this review.

Official contract checked 2026-09-16: portable root components & OpenAI overlay selection follow [OpenAI plugin packaging](https://developers.openai.com/plugins/build/plugins). Bundled hooks require discovery plus trust of current hook definition; plugin enablement alone does not trust hooks.

## Open closure gates

| Gate | Evidence & required closure |
|---|---|
| Native plugin install/enable | Reviewed activation reconciles MCP/hooks but has no native plugin discovery/install/enable/trust repair transaction. Prove each native host plugin installed/enabled independently. |
| Declared transport | Configuration get/match is not authenticated MCP initialize, tool/resource discovery & status invocation against exact installed generation. Exercise real HTTP & stdio projections. |
| Native hooks | Candidate build does not pass hooksManifestPath into portable-core assembly; no root hooks manifest is present. Prove packaged Codex SessionStart discovery, current hook-definition trust & one real orientation insertion, separately from global-hook reconciliation. |
| Candidate proof | Candidate check invokes membrane-client hook --help; candidate-handoff test substitutes Node as executable. Neither establishes real MCP, native hook delivery or plugin enablement. Final overlay/version stamping occurs after portable-core validation. Validate final packaged artifact. |
| Readiness scope | Engine-only activation can produce service ready with clients empty. Valid engine-only state must not be reported as any host/plugin ready; normal activation defaults an empty host selection to supported clients. |
| macOS | Bearer provisioning & non-Windows PATH mutation contain no-op branches. Prove macOS authenticated transport, stable installed executable resolution & rollback. |
| Other hosts | Cursor/Windsurf actual-host stdio acceptance remains open; Antigravity declaration uses bare membrane-client & needs proven installed resolution. No Devin proof can be inferred from these hosts. |
| Lifecycle/install recovery | Qualify repeated activation, one registration/engine, owner release, interrupted update, rollback & uninstall through actual installed hosts. |

## Focused verification

Command:

```text
node --test tests/onboarding/getting-started-consistency.test.mjs scripts/qualification/cases/psh-windows.test.mjs scripts/qualification/cases/mem-windows.test.mjs scripts/qualification/install-release.test.mjs apps/membrane-hub/tests/candidate-handoff.test.mjs scripts/qualification/installer-nsi-activation-contract.test.mjs
```

Observed: **65 pass / 1 fail / 66 total**, no skipped tests. Failure: `windows-r5 registry resolves every PSH row to its exact module export`, requiring absent `D:/Claude/review/windows-r5/windows-acceptance.json`. That requirement also exists at base HEAD; the missing fixture predates this slice. Fixture/fake-caller test success is not actual-host acceptance. No candidate was built or installed by this review.

## Atom disposition

MEM-009/031/045/046/048/050 now record PARTIAL local implementation. MEM-007/016/017/067 & MEM-057–060 retain partial/open status with explicit residual gates. Existing MCP-focused evidence remains historical; no blanket downgrade of unrelated delivered transport mechanisms. MEM-069 remains unproven independently. Qualification stays open; no PASS/RELEASED claim is created.

## Reviewed source fingerprints

| Source | SHA-256 |
|---|---|
| mcp.json | 10c41e24a31082dfeaf39f2f4c759f6db8b50ff0b66e0d93011009fd3d5648af |
| .claude-plugin/plugin.json | 3982caa7cac9312fec37b6cc251b02a25ac23551eaddd5f56478c303cb2357c0 |
| .codex-plugin/plugin.json | efd2183fe62e5d56a90bcb3ec22ff998ed9c0e64da44741fa295f49056805a64 |
| .antigravity-plugin/mcp_config.json | 5d711b5b884dcd89419e4995f79c5c395300c0d250955094f250b7c3e5edfb51 |
| engine/crates/membrane/src/activation.rs | 39017350de126a4172c6b160e3f55b4a9b0233b5976324545ec7a29f005bd698 |
| apps/membrane-hub/scripts/release-build-candidate-windows.mjs | 51fc05caf88454ccba5b80fca08362e8d3353983e83a3d6c85b6e8bb80d00311 |
| apps/membrane-hub/scripts/release-check-candidate-windows.mjs | 992cdfcd29c2d400e360dbcfe7ce3ca79863f5c66913ed87f3513a6c874c90e9 |
| apps/membrane-hub/tests/candidate-handoff.test.mjs | 8c883325d1f0eca315235bedc94f86ef6c4ff23be097bd162deb6b4a9ee8ddf2 |
| scripts/ci/check-release-identity.mjs | 644e7937602e80e1bca187c7301496bc08de1f80a51f0a20538b342e92264811 |
| plugin.json | 7a8f61ed6a46521c0f201c4f481c74a255a3cae2dbadc09e730d835ccd33d400 |
