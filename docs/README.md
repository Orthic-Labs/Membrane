# Docs index

Start here. This folder is organized around reader intent, not history.

## Architecture and migration authorities

Exactly three architecture/planning documents are normative for the current convergence:

- [MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md](MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md) — Membrane architecture/implementation authority.
- [subsystems/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md](subsystems/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md) — pre-merge Blueprint authority; moves with Blueprint to `blueprint/docs/` during the monorepo migration.
- [plans/2026-08-19-monorepo-merge-and-subsystem-rename.md](plans/2026-08-19-monorepo-merge-and-subsystem-rename.md) — physical merge/name migration authority only.

Derived navigation/reference docs live under [subsystems/](subsystems/):

- [SYSTEM.md](subsystems/SYSTEM.md) — Membrane parent-system map.
- [cortex.md](subsystems/cortex.md) — durable-knowledge reference.
- [guide.md](subsystems/guide.md) — document-navigation/index reference.
- [adapt.md](subsystems/adapt.md) — learning/proposal reference.
- [push.md](subsystems/push.md) — reversible-reduction reference.

Derived subsystem docs cannot override the three authorities above.

## Generated (code-grounded, never hand-edit)

Regenerate with the current productization tooling under `scripts/tools/productization/`.

- [product.md](product.md) — what landed Membrane currently is and does.
- [architecture.md](architecture.md) — landed components, flows, interfaces.
- [operations.md](operations.md) — run and verify the product-truth surface.
- [protocol.md](protocol.md) — landed MCP/tool contract and behavior.
- [product-truth.md](product-truth.md) — raw derived facts backing the generated docs.

Generated/runtime docs may continue to contain legacy `Cortex`, `Crypt`, or `Spine` implementation names until the corresponding code migration lands. Do not hand-edit them to pretend implementation has moved.

## Get started

- [getting-started.md](getting-started.md)
- [install.md](install.md), [install-npm.md](install-npm.md), [install-oci.md](install-oci.md), [install-registry.md](install-registry.md)
- [uninstall.md](uninstall.md)
- [doctor.md](doctor.md)
- [pricing.md](pricing.md), [support-policy.md](support-policy.md), [support-matrix.md](support-matrix.md)
- [privacy.md](privacy.md)

## Core runtime/reference surfaces

- [memory/](memory/) — landed memory lifecycle/runtime material.
- [hub/](hub/) — Hub integration/facade material.
- [agent-rules.md](agent-rules.md) — rules for agents working in this repository.
- [cli/](cli/), [clients/](clients/), [sdk/](sdk/), [providers/](providers/), [reference/](reference/)
- [compatibility/](compatibility/), [compression/](compression/), [evaluation/](evaluation/), [benchmarks/](benchmarks/)
- [installation/](installation/), [migrations/](migrations/), [operations/](operations/), [protocol/](protocol/)
- [security/](security/), [legal/](legal/), [troubleshooting/](troubleshooting/), [workflows/](workflows/)
- [fleet/](fleet/), [team/](team/), [membrane/](membrane/), [runs/](runs/), [release/](release/)

## Design, history and provenance

Design rationale, architecture history, research, and point-in-time state remain evidence/provenance. They are not parallel implementation authorities.

## Plans

The active Blueprint/Cortex monorepo/name migration is the canonical migration plan linked above. Other plans remain scoped work/provenance and cannot override the canonical doctrines.
