Cortex for Windows

- Per-user installer (Inno Setup): installs to %LOCALAPPDATA%\Orthic\Cortex,
  adds a user PATH entry, records uninstall metadata.
- The watcher can be registered as a per-user background scheduled task
  (optional, hidden console).
- All payloads are Authenticode-signed with SHA-256 and RFC3161 timestamps
  in the release pipeline.
- No admin rights are required.

Build:  pwsh scripts/release/windows/build-installer.ps1 -Staging <dir> -OutDir <dir>
Verify: pwsh scripts/release/windows/verify-signatures.ps1 -Root <dir>
Uninstall check: pwsh release/windows/uninstall-check.ps1
