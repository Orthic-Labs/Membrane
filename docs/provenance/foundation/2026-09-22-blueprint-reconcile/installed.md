# Installed Blueprint reconciliation — PASS

Follow-up to [initial evidence](evidence.md). Source `542f0464344d97a718152de651a5fbedc90dded2` passed canonical Windows internal-unsigned build/install qualification. [Hash-bound receipt extract](installed.json).

- Installer SHA-256: `5af7463b274cbeb712073eca2c93e33c6216873387f876f22d33dac8ffbaefd0`.
- Installed generation: `sha256:29012b9d9e12b08fe87877f8e74b86699bdf495a95c0c42c8461428c25c0fd95`.
- Resident source mutation, query & commit-only freshness: PASS.
- Hub-off explicit Blueprint, MCP, lifecycle, repair/upgrade & uninstall checks: canonical lane PASS.
- Post-install main repository status: `fresh`, sealed HEAD `542f0464344d97a718152de651a5fbedc90dded2`, graph `xxh128:e9bb29178026f9066d97498b24072037`.

Outer disposable launcher misclassified success because PowerShell returned null for its process object's exit code. Canonical lane emitted PASS & its own completed qualification receipt; no qualification check was bypassed. Final install left Hub off; tray was restarted through installed `--login-launch` after readback.

Stale delivery was independently proven by actual Codex Pull on preceding installed candidate: 16 labelled Blueprint blocks with coherent resolved graph receipts. Follow-up changes binary file facts & retained startup diagnostics; it does not alter Pull.

This closes the observed stale-delivery & binary-blocked watcher defects at internal Windows boundary. RELEASED qualification & broader product lifecycle closure remain separate; no globally closed atom is claimed.
