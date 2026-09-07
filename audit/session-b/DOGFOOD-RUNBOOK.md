# Session B dogfood runbook

## Identity and prerequisites

Target source: `a4e0a9e02c64e5439cdec289a160b748f5e5686c` in the detached task-owned worktree. Required JavaScript layer: Node satisfying `package.json` plus repository dependencies installed by the workspace bootstrap (not ad hoc). Required native layer: revision-compatible prebuilt `membrane` and Hub runtime binaries, supplied as `MEMBRANE_TEST_BIN` and `MEMBRANE_TEST_HUB_RUNTIME_BIN`, with exact build/source identity. Installed layer additionally requires an isolated installer-owned current root and host; it must never use the active user installation.

The audit cloud currently has Node `v20.20.2`, no repository `node_modules`, no compatible native binary, no configured remote/main ref, and pnpm 11.24.0 cannot run on this Node because it requires Node >=22.13 and `node:sqlite`. Per the no-ad-hoc-toolchain/no-build policy, do not install around these prerequisites.

## Allowed commands

From repository root, after the normal workspace bootstrap provides the pinned toolchain/dependencies:

```sh
node --check audit/session-b/tests/public-boundary-dogfood.test.mjs
node --test audit/session-b/tests/public-boundary-dogfood.test.mjs
sha256sum -c audit/session-b/ARTIFACT-SHA256SUMS
```

The test creates only `membrane-dogfood-b-*` directories beneath the OS temporary directory and removes them in `finally`. It starts only the retained JavaScript stdio MCP server and bounds it to seven seconds. Port 65531 is deliberately expected to have no Hub listener. Do not run if that port is assigned in the test namespace.

## Native-blocked procedure

Do **not** build. Obtain a legitimately produced prebuilt artifact whose receipt names the exact target SHA and platform. Copy neither it nor user state into this repository; point the test-only environment to it. If exact identity, tray-owned isolated launch, provider credentials, or test-owned data roots cannot be proven, retain `BLOCKED`/`NOT_RUN`. A manually started daemon or different revision cannot qualify this target.

## Fixture variants

Copy `fixtures/repo-alpha`, `fixtures/repo-beta`, `fixtures/documents`, and `fixtures/knowledge` into a fresh task-owned temp root for each mutable scenario. Verify original bytes against `FIXTURE-SHA256SUMS` before and after. Cold/warm/edit/restart cases must use the same authored manifest; normalize only declared volatile timestamps/IDs.

## Result policy

Run oracle negative controls separately. Their success is harness evidence only. Update result statuses only from captured public-boundary output, retaining raw bounded logs and exact artifact/source identity. Never change the frozen oracle to match observed output.

## Complete native suite

`tests/native-public-boundary-dogfood.test.mjs` contains executable public-MCP cases B02 through R01. Run it with:

```sh
MEMBRANE_TEST_BIN=/receipt-bound/exact/membrane \
MEMBRANE_TEST_HUB_RUNTIME_BIN=/receipt-bound/exact/hub-runtime-test-host \
node --test audit/session-b/tests/native-public-boundary-dogfood.test.mjs
```

`L02` additionally requires `MEMBRANE_DOGFOOD_INSTALLED_BINDING` naming an isolated installer-owned binding. Without these variables each case emits a precise prerequisite-based skip; it never fabricates a pass. The suite negotiates MCP over the actual binary's `stdio-mcp` surface, calls the advertised operations, and checks independently authored fixture facts/forbidden evidence. Any schema or dispatch mismatch with the real binary is an honest test failure.
