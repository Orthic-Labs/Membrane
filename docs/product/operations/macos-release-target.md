# macOS release target

Owner decision — Adrian D'souza, 2026-08-22:

- While Membrane remains in testing, ship macOS releases for Apple Silicon only (`aarch64-apple-darwin`).
- Do not spend build time on universal or Intel artifacts during this phase.
- Resume universal macOS releases only after a new explicit owner decision.

Membrane Hub release config, artifact names, receipts, runtime inventory, & manifest must identify arm64 truthfully.
