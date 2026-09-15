# Candidate 4 — build & install result

Completed requested steps 1 & 2 only.

- Frozen source: main base 3dbb0d8bcfe6e077e4c3a4146fbf10d1a731157b plus recorded working-tree changes. All 1342 frozen product files matched after build.
- Engine generation: sha256:ac44820f5b90dc16b573cba5fccca5e27525ae8c98f451261b98b030f2af40c1.
- One additional unsigned build invoked; exit 0. Existing packaging script runs two NSIS passes to preserve raw Hub bytes; no additional build invocation or retry.
- Installer: Membrane_Hub_0.1.24_x64-setup.exe, 205362705 bytes.
- Installer SHA256: 7b4927e7539ba316795f69c5ab3c39375acc1ca7f0d433a3b610bcdc88747701. Matches candidate.json.
- Silent installation exit 0. Stop, extraction, verification, junction cutover, registration & binding-only activation all passed.
- Stable current: C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current → versions/0.1.24.
- Installed release.json equals staged candidate release.json; all 157 payload file hashes verified.
- Installed stateless cli build-info reports exact frozen generation & base source commit.
- Post-install snapshot: no Membrane processes; no Membrane Engine scheduled task.

Evidence: pre-build.md, frozen-files.json, source-identity.json, artifact-verification.json, candidate.json, release.json, .tmp-candidate4-build.log, install-result.json, install-0.1.24.log, installed-build-info.json, installed-verification.json & post-install-state.json in this directory.

No ownership matrix, crash recovery, Pull/Push/hooks, performance qualification, commit, push or publication performed. Source remains uncommitted. This result establishes build/install identity only.
