# Hub handoff (CU-H01)

Orthic owns its desktop installer. Membrane produces a signed portable add-on
containing only `membrane`, `cortex-service`, icon, legal files, and its sealed
manifest. Existing Hub sources remain until RightKit publication, both
platform uploads, and remote Orthic adoption have receipts.

## Cortex-service start-up after handoff

- **Hub-spawn via manifest (while Hub runs):** Orthic Hub reads the atomic
  v1 manifest at `~/.orthic/hub/products.d/membrane.json`, verifies its
  compatible Hub range, then launches the declared `cortex-service` argv with
  its inline authentication token.

- **Headless/standalone:** `membrane service run` (existing CLI entry point `engine/crates/membrane/src/cli.rs`) starts cortex-service headless/standalone for servers/CI/SSH-only hosts without a Hub.

- **No OS registration:** No launchd/systemd/Task Scheduler registration for cortex-service itself. CU-H05 removes the supervisor's own OS-service auto-registration capability; cortex-service was never separately registered, only spawned as the Hub's child. Standalone use is an explicit `service run` invocation, not a persistent OS service.
