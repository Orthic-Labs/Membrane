# macOS signing

RightKit `right-release` owns macOS packaging, Developer ID signing,
notarization, stapling, verification, hardening, sealing, and publication.
Blueprint does not carry signing credentials or reproduce that pipeline in CI.

From the clean primary checkout, build without uploading:

```sh
pnpm release:build:mac
```

The successful command prints an exact release ID and sealed path under
`.right-release/sealed/`. Verify the notarized installer on a clean host through
install, init, query, MCP, update, rollback, and uninstall before upload.

Upload only the exact sealed release requested for publication:

```sh
pnpm exec right-release upload --release <release-id> --platform mac --tier patch
```

`patch` publishes the public installer lane; `update` is a distinct licensed
updater lane. Both fail closed on trust, hardening, commit, or checksum drift.

See the workspace release-signing runbook and `docs/operations/uninstall.md`.
