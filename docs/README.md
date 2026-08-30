# Docs index

Start here. This folder is organized around reader intent, not history.

## Architecture authorities

[Current architecture](current/architecture/) is one normative set:

- [Membrane parent architecture](current/architecture/membrane.md)
- [Blueprint architecture](current/architecture/subsystems/blueprint.md)
- [Adapt architecture](current/architecture/subsystems/adapt.md)
- [Ledger architecture](current/architecture/subsystems/ledger.md)
- [Cross-subsystem evidence contracts](current/architecture/cross-subsystem-evidence.md)
- [CodeRight integration](current/architecture/integrations/coderight.md)

[Atomic capability canons](current/atoms/) track landed state by subsystem. [Pending](pending/) indexes only open atoms & supporting specs. Superseded, derived, & historical material lives under [archive](archive/), with deleted-path dispositions in its [preservation map](archive/PRESERVATION.md).

Visible native tray owns resident lifecycle. Tray-owned headless daemon executes runtime. Hub dashboard is on demand.

## Generated (code-grounded, never hand-edit)

Regenerate with the current productization tooling under `scripts/tools/productization/`.

- [product.md](product.md) — what landed Membrane currently is and does.
- [architecture.md](architecture.md) — landed components, flows, interfaces.
- [operations.md](operations.md) — run and verify the product-truth surface.
- [protocol.md](protocol.md) — landed MCP/tool contract and behavior.
- [product-truth.md](product-truth.md) — raw derived facts backing the generated docs.

Generated/runtime docs use final greenfield identities: Blueprint for repository truth, Cortex for durable knowledge, Ledger for document navigation, Adapt for learning, & Push for reduction.

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
- [compatibility/release-channels.md](compatibility/release-channels.md), [evaluation/](evaluation/), [benchmarks/](benchmarks/)
- [installation/](installation/), [operations/](operations/), [protocol/](protocol/)
- [security/](security/), [legal/](legal/), [troubleshooting/](troubleshooting/), [workflows/](workflows/)
- [fleet/](fleet/), [team/](team/), [membrane/](membrane/), [release/](release/)

## Current work & records

- Current architecture authorities are listed above; capability atom canons live under [current/atoms/](current/atoms/).
- Open work lives only under [pending/](pending/).
- Qualification & release receipts live under [evidence/](evidence/).
- Superseded documents & historical research live under [archive/](archive/), including [legacy source corpus](archive/research/legacy-source-corpus/).
