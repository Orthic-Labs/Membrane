# Installation

Cross-repository index for install/bootstrap paths and the installation contract. Kept at their
original top-level paths (`docs/install*.md`) because packaged npm artifacts reference those exact
paths in published prose; this directory holds the underlying contract and root documents.

- [../install.md](../install.md) — Membrane Hub's native install/runtime authority
- [../install-npm.md](../install-npm.md) — `@membrane/membrane` npm bootstrap loader
- [../install-oci.md](../install-oci.md) — optional headless Docker/OCI image
- [../install-registry.md](../install-registry.md) — MCP Registry metadata (MBR-907)
- [contract.md](contract.md) — installation manifest and IPC handshake contract (MBR-105)
- [roots.md](roots.md) — the four durable Membrane Stable Roots directories (MBR-106)
