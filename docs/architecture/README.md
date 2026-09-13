# Membrane current architecture

This directory is sole current architecture set. Product capability state lives in
[`../canon/`](../canon/); open work lives in [`../pending/README.md`](../pending/README.md);
superseded/derived material lives in [`../archive/`](../archive/).

## Canonical authorities

1. [Membrane parent architecture](membrane.md)
2. [Blueprint architecture](subsystems/blueprint.md)
3. [Adapt architecture](subsystems/adapt.md)
4. [Ledger architecture](subsystems/ledger.md)
5. [Cross-subsystem evidence contracts](cross-subsystem-evidence.md)
6. [CodeRight integration](integrations/coderight.md)

## Current supporting architecture

- [Single-instance Membrane & harness connections](single-instance-membrane.md) — Hub-or-harness lifetime, historical source trace, migration & acceptance; not an installed-behavior claim.
- [Tray-owned resident lifecycle decision](adr/tray-daemon-process.md)
- [Tray–daemon runtime contract](runtime/tray-daemon-contract.md)
- [Live Diagnostics](live-diagnostics.md)
- [MCP threat model](security/mcp-threat-model.md)
- [Update admission](security/update-admission.md)
- [Current-state manifest](current-state-manifest.json)

Hub always starts/adopts & holds one Membrane engine. Harness access also starts/adopts & holds it with Hub on or off. Hub off with no harness access stops engine/daemon after bounded drain; no independent engine autostart or periodic restart task is permitted. Hub's optional login startup launches Hub, which starts Membrane. Background authority remains separate from ordinary harness access. [Decisions 21 & 24](adr/2026-09-12-context-system-decisions.md) & [execution boundary](execution-lifecycle-boundary.md) govern this lifetime.
