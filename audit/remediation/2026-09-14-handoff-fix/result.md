# Installed startup/handoff fix — verified

Source generation: sha256:79178a91e787497d71c11d62b96616cd8fc3cb1a7ce91108eee8ef46ee8c3fae. One authorized installer build completed exit 0; silent install exit 0. Frozen input hashes unchanged before installation; all installed release-manifest payload hashes matched. Installed client identity matched generation. Installer SHA256 recorded in installer-hash.json.

Changes: file-backed activation diagnostics avoid waiting for inherited stderr EOF. New activate --engine-only retains installed identity/singleton checks while skipping installer binding/PATH/credential/hook reconciliation. Client polls signed ownership while activation runs; ready clients do not wait for competing activation's 15-second lock timeout. Owned activation controls are reaped within deadline. No changes to scheduled startup policy.

Focused checks: 16 forwarding-client tests, including activation exit while stderr-inheriting descendant remains alive; 95 control-library tests; final 17 dispatch tests including engine-only/conflicting binding-only flags. All passed. Captured under previous fresh-candidate evidence directory as handoff-* logs.

Installed qualification, original 20-second RPC timeout:
- First five-way cold startup: 5/5 success, about 5.2 seconds each.
- Repeat cold startup: 5/5 success, 2759–2775 ms each.
- One resident engine in both runs; closing four clients preserved survivor.
- Forced engine termination recovered through surviving harness in both runs; MCP tools/list usable after recovery.
- Final owner release drained & stopped engine cleanly in both runs. Engine log contains engine_stopped, no shutdown timeout for these harness-only runs.
- Functional smoke: Pull, Blueprint status, Push exact UTF-8/hash/Cortex readback, SessionStart & SessionEnd hooks passed.
- Final global Membrane process snapshot empty.

Scope: startup capture/handoff fix verified installed. Existing Hub-indexing shutdown timeout & concurrent Pull performance defects were not changed or claimed resolved by this work. No commit/push. Evidence: harness-lifetime.json, harness-repeat.json, functional.json, summary.json, final-processes.json, pre-build.md, build.log, release.json, installed-build-info.json.
