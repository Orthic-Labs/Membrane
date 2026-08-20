# Windows signing

RightKit `right-release` owns Windows installer construction, Azure Trusted
Signing, signature verification, hardening, sealing, and publication. Cortex
does not carry signing credentials or reproduce that pipeline in GitHub Actions.

From the clean primary checkout, build without uploading:

```powershell
pnpm release:build:win
```

The successful command prints an exact release ID and sealed path under
`.right-release/sealed/`. Verify the signed installer on a clean non-admin host
through install, init, query, MCP, update, rollback, and uninstall before upload.

Upload only the exact sealed release requested for publication:

```powershell
pnpm exec right-release upload --release <release-id> --platform win --tier patch
```

`patch` publishes the public installer lane; `update` is a distinct licensed
updater lane. Both fail closed on signature, hardening, commit, or checksum drift.

See the workspace release-signing runbook and `docs/operations/uninstall.md`.
