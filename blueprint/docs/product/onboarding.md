# Desktop onboarding

Status: implemented and roundtrip-tested; not yet published.

Onboarding installs CLI, MCP registration, service, tray, and update trust
root through RightRelease-owned installers. First run never enrolls a
repository without explicit user action.

`cortex init` records reversible host edits, MCP registration, and watcher
enrollment in an integrity-sealed install state. `cortex uninstall` restores
host bytes, removes only enrollment created by init, and preserves graph data
and the shipped update trust root. Desktop packaging requires Node 22.22.3+.
