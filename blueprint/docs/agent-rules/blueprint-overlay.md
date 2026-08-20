# Blueprint overlay — in-repo seam authority

Before sealing any contract touching hub, watcher lifecycle, Blueprint↔Membrane API, or peer-service discovery, read `docs/FEDERATION-CONTRACT.md`, `docs/operations/service.md`, and `docs/architecture.md`, then run `node scripts/ci/check-seam-conformance.mjs`.

Keep Blueprint seam authority self-contained. Never require a workspace-external plan as a local prerequisite.
