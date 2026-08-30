# Docs index

Start here. This folder is organized around reader intent, not history.

## Architecture authorities

[Current architecture](architecture/) is one normative set:

- [Membrane parent architecture](architecture/membrane.md)
- [Blueprint architecture](architecture/subsystems/blueprint.md)
- [Adapt architecture](architecture/subsystems/adapt.md)
- [Ledger architecture](architecture/subsystems/ledger.md)
- [Cross-subsystem evidence contracts](architecture/cross-subsystem-evidence.md)
- [CodeRight integration](architecture/integrations/coderight.md)

[Atomic capability canons](canon/) track landed state by subsystem. [Pending](pending/) indexes only open atoms & supporting specs. Superseded, derived, & historical material lives under [archive](archive/), with deleted-path dispositions in its [preservation map](archive/PRESERVATION.md).

Visible native tray owns resident lifecycle. Tray-owned headless daemon executes runtime. Hub dashboard is on demand.

## Generated runtime truth (code-grounded, never hand-edit)

Regenerate with the current productization tooling under `scripts/tools/productization/`.

- [Product overview](product/README.md) — what landed Membrane currently is and does.
- [Runtime architecture truth](architecture/runtime-truth.md) — landed components, flows, interfaces.
- [Operations](product/operations/README.md) — run and verify the product-truth surface.
- [Protocol](reference/protocol/README.md) — landed MCP/tool contract and behavior.
- [Product truth](reference/product-truth.md) — raw derived facts backing generated docs.

Generated/runtime docs use final greenfield identities: Blueprint for repository truth, Cortex for durable knowledge, Ledger for document navigation, Adapt for learning, & Push for reduction.

## Get started

- [Getting started](product/getting-started.md)
- [Install](product/installation/install.md), [npm](product/installation/npm.md), [OCI](product/installation/oci.md), [registry](product/installation/registry.md)
- [Uninstall](product/installation/uninstall.md)
- [Doctor](product/troubleshooting/doctor.md)
- [Pricing](product/support/pricing.md), [support policy](product/support/policy.md), [support matrix](product/support/matrix.md)
- [Runtime privacy](product/legal/runtime-privacy.md)

## Layout

- [architecture/](architecture/) — current normative architecture (authorities above).
- [canon/](canon/) — atomic capability canons: landed state by subsystem.
- [pending/](pending/) — the only index of open work.
- [product/](product/) — operator/user-facing docs: [installation](product/installation/), [compatibility](product/compatibility/), [operations](product/operations/), [troubleshooting](product/troubleshooting/), [hub](product/hub/), [memory](product/memory/), [workflows](product/workflows/), [fleet](product/fleet/), [legal](product/legal/).
- [reference/](reference/) — contracts & developer surfaces: [protocol](reference/protocol/), [sdk](reference/sdk/), [cli](reference/cli/), [clients](reference/clients/), [providers](reference/providers/), [examples](reference/examples/), [evaluation](reference/evaluation/), [benchmarks](reference/benchmarks/), [security](reference/security/), [release](reference/release/), [team](reference/team/), [adr](reference/adr/).
- [research/](research/) — non-authoritative research inputs ([legacy source corpus](research/legacy-source-corpus/)).
- [research/rightcontext-history/](research/rightcontext-history/) — recovered RightContext/MemRight architecture lineage; historical input, never current authority.
- [evidence/](evidence/) — qualification & release receipts.
- [provenance/](provenance/) — frozen migration records.
- [membrane/](membrane/) — vendored machine data (capability matrix, federation freeze).
- [archive/](archive/) — the few superseded documents worth retaining; everything else lives in git history.
- [agent-rules.md](agent-rules.md) — machine-loaded repository overlay; source infrastructure, not product documentation.
