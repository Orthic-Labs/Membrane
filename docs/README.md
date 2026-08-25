# Docs index

Start here. This folder is organized around reader intent, not history.

## Architecture authorities

Exactly two documents are normative:

- [MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md](subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md) — Membrane architecture/implementation authority.
- [BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md](subsystems/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md) — Blueprint repository-truth authority.

Derived navigation/reference docs live under [subsystems/](subsystems/):

- [SYSTEM.md](subsystems/SYSTEM.md) — Membrane parent-system map.
- [cortex.md](subsystems/cortex.md) — durable-knowledge reference.
- [ledger.md](subsystems/ledger.md) — document registry/navigation/index reference (formerly guide.md).
- [adapt.md](subsystems/adapt.md) — learning/proposal reference.
- [push.md](subsystems/push.md) — reversible-reduction reference.

Derived subsystem docs cannot override these authorities.

Membrane Hub is sole runtime, desktop install, release-build, publication, &
cleanup authority. No external product manifest, add-on handoff, or retired
installer path is active.

## Generated (code-grounded, never hand-edit)

Regenerate with the current productization tooling under `scripts/tools/productization/`.

- [product.md](product.md) — what landed Membrane currently is and does.
- [architecture.md](architecture.md) — landed components, flows, interfaces.
- [operations.md](operations.md) — run and verify the product-truth surface.
- [protocol.md](protocol.md) — landed MCP/tool contract and behavior.
- [product-truth.md](product-truth.md) — raw derived facts backing the generated docs.

Generated/runtime docs use final greenfield identities: Blueprint for repository truth, Cortex for durable knowledge, Guide for document navigation, Adapt for learning, & Push for reduction.

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
- [compatibility/release-channels.md](compatibility/release-channels.md), [compression/](compression/), [evaluation/](evaluation/), [benchmarks/](benchmarks/)
- [installation/](installation/), [operations/](operations/), [protocol/](protocol/)
- [security/](security/), [legal/](legal/), [troubleshooting/](troubleshooting/), [workflows/](workflows/)
- [fleet/](fleet/), [team/](team/), [membrane/](membrane/), [release/](release/)

## Design & research

Current security/product decisions live under [design/](design/). Research & historical evidence stay under [research/](research/) & never override subsystem authorities.
