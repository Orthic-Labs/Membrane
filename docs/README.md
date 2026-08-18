# Docs index

Start here. This folder is organized around reader intent, not history.

## Generated (code-grounded, never hand-edit)

Regenerate with `node tools/productization/generate-product-truth.mjs`.

- [product.md](product.md) — what Membrane is and does
- [architecture.md](architecture.md) — components, flows, interfaces
- [operations.md](operations.md) — run and verify the product-truth surface
- [protocol.md](protocol.md) — MCP tool contract and behavior
- [product-truth.md](product-truth.md) — raw derived facts backing the above

## Get started

- [getting-started.md](getting-started.md)
- [install.md](install.md), [install-npm.md](install-npm.md), [install-oci.md](install-oci.md), [install-registry.md](install-registry.md)
- [uninstall.md](uninstall.md)
- [doctor.md](doctor.md) — diagnostics
- [pricing.md](pricing.md), [support-policy.md](support-policy.md), [support-matrix.md](support-matrix.md) ([json](support-matrix.json))
- [privacy.md](privacy.md)

## Core surfaces

- [memory.md](memory.md) — memory lifecycle, plus detail in [memory/](memory/)
- [hub.md](hub.md) — hub facade overview, plus detail in [hub/](hub/) and [hub-handoff.md](hub-handoff.md)
- [MEMBRANE-IMPLEMENTATION-GUIDE.md](MEMBRANE-IMPLEMENTATION-GUIDE.md) — canonical implementation authority
- [agent-rules.md](agent-rules.md) — rules for agents working in this repository

## Reference subtrees

- [cli/](cli/), [clients/](clients/), [sdk/](sdk/), [providers/](providers/), [reference/](reference/)
- [compatibility/](compatibility/), [compression/](compression/), [evaluation/](evaluation/), [benchmarks/](benchmarks/)
- [installation/](installation/), [migrations/](migrations/), [operations/](operations/), [protocol/](protocol/)
- [security/](security/), [legal/](legal/), [troubleshooting/](troubleshooting/), [workflows/](workflows/)
- [fleet/](fleet/), [team/](team/), [membrane/](membrane/), [runs/](runs/), [release/](release/)

## Design, history & internal state

Design rationale, architecture history, and point-in-time internal state live in
[design/](design/) — not reader entry points, kept for provenance and traceability.

## Plans

Active and historical work plans live in [plans/](plans/) (owned separately from the rest of `docs/`).
