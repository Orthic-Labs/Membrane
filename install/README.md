# Membrane Windows installation

Membrane ships as a signed Windows installer built from `apps/membrane-hub`.
Installed runtime contains Membrane Hub, native `membrane` & `cortex`
sidecars, & one pinned Blueprint runtime component. Blueprint remains an
independently owned, separately bounded package; its staged runtime artifact is
reusable for later standalone channels. It contains no Membrane-owned Python
runtime.

One installer makes Blueprint available through its installed command. Hub owns
its resident service/watcher over canonical per-user Windows named pipe;
Hub-off access is bounded one-shot only.

Installed install/upgrade/uninstall & Blueprint qualification is implemented by
`scripts/qualification/install-release.ps1` plus Blueprint lifecycle runner.

This seal qualifies only signed Windows installer locally. External Blueprint
provisioning is outside this release.

Retired `install/workspace` & `dist/install/workspace` Python projections are
not release inputs.
