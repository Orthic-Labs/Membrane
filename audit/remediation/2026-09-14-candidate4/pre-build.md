# Candidate 4 — pre-build clearance, 2026-09-14

Scope authorized: steps 1 & 2 only — freeze source, one additional unsigned installer build, install exact candidate, verify identity. No lifetime/functional/performance qualification, commit, push or publication. Earlier allowance 3/3 exhausted; this is the one additional build explicitly authorized by “go on, do 1 and 2 only”.

## Frozen candidate

Base commit: 3dbb0d8bcfe6e077e4c3a4146fbf10d1a731157b (dirty working tree intentionally included).
Engine generation/source digest: ac44820f5b90dc16b573cba5fccca5e27525ae8c98f451261b98b030f2af40c1 (1006 files).
All 1342 product source/config paths frozen with SHA256 in frozen-files.json; source-identity.json is canonical packaging identity. No source edits after freeze.

## Traced risk paths

| Stage / risk | Source path & mitigation / evidence |
| --- | --- |
| Feature-specific compile failure | build-frontend.mjs builds cortex + membrane + membrane-client using membrane-runtime/fastembed. Managed all-target cargo check for membrane-runtime + membrane with fastembed passed before build. Prior source validation: 99 Rust + 92 repository tests. |
| Mismatched engine/client/tray payload | release-build-windows.mjs stages managed engine binaries, builds tray with fastembed, then builds Hub & NSIS. release-identity.mjs hashes dirty engine source; installed build-info must match frozen digest, not HEAD alone. |
| Accidental full qualification | Do not run release:local:win:unsigned wrapper: it automatically runs installed qualification. Use documented build-only release:build:win:unsigned, then exact NSIS /S. |
| Locked old payload / concurrent engine | Installer .install-lock precedes explicit deactivate; stop loop checks all product EXEs, bounded retries. No Membrane product processes or scheduled task observed at preflight. Recheck before install. No ad hoc deletion or separate installer. |
| Extraction failure | installer.nsi extract-version-tree logs exact failing payload, retries bounded; then verify-version-tree checks required files. Historical failure reviewed; same-version overwrite remains in-place. AV/filesystem interference is diagnosed from exact log rather than consuming another build. |
| Junction cutover failure | .current-next staged before current rename; nonrecursive junction removal & rollback path. Stable destination fixed to LOCALAPPDATA/Orthic Labs/Membrane/current. Prior current is a junction. |
| Binding lock / unintended engine start | installer invokes membrane.exe activate --bindings-only. activate_bindings calls activate_with_residency(options,false); OS-backed activation lock handles abandoned owner; control image excluded from tree-release probe. Installer removes old scheduled task, binds after cutover, no watcher health gate. 14 focused installer contract tests passed. |
| Payload/source drift | Compare frozen files after build, candidate.json artifact SHA with actual installer, release generation with source-identity.json, installed build-info & all release.json file hashes after install. |
| Disk pressure | C free ~855 GB; D free ~1.51 TB at preflight. |

Cleanup manifest & invocation graph checks both pass, zero errors. No native-only seal. Release-feature check passed with existing warnings. No active managed job remains from this task before build starts. Existing installer header comments contain retired autostart wording; executable registration path above is authoritative.

Build command: pnpm --dir apps/membrane-hub run release:build:win:unsigned.
Install command: exact resulting installer /S, hidden, wait for exit; no qualify script.
Failure policy: preserve output & diagnose exact failed stage. No second additional build authorized.
