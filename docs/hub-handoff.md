# Hub handoff (CU-H01)

Migration verified: `git ls-remote https://github.com/Orthic-Labs/orthic HEAD` → `05174f95833ccacf925ba5e8cc77d308c2f867d9` (already-landed extraction target per hub-strip contract §1). The chassis that lived at `apps/membrane-hub/**` (224 files / 3,178 LOC) now lives at `github.com/Orthic-Labs/orthic` with history preserved per seam D-S13. This repo's local copy is retained until the next window to keep `cargo test -p membrane-runtime` (422) and `pnpm tauri build` replacement checks green; deletion is `git rm -r apps/membrane-hub` in the next dispatch, gated on the same pre-flight.

## Crypt-service start-up after handoff

- **Hub-spawn via manifest (while Hub runs):** Orthic Hub spawns crypt-service via the manifest's `serviceStart` argv (`install/workspace/orthic_manifest.py` product `~/.orthic/hub/products.d/membrane.json`, CU-H02). The Hub reads `serviceStart` and launches crypt-service as its child per `engine/crates/membrane/src/main.rs:210-227`.

- **Headless/standalone:** `membrane service run` (existing CLI entry point `engine/crates/membrane/src/cli.rs`) starts crypt-service headless/standalone for servers/CI/SSH-only hosts without a Hub.

- **No OS registration:** No launchd/systemd/Task Scheduler registration for crypt-service itself. CU-H05 removes the supervisor's own OS-service auto-registration capability; crypt-service was never separately registered, only spawned as the Hub's child. Standalone use is an explicit `service run` invocation, not a persistent OS service.
