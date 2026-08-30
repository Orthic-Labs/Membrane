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

- [Tray-owned resident lifecycle decision](adr/tray-daemon-process.md)
- [Tray–daemon runtime contract](runtime/tray-daemon-contract.md)
- [Live Diagnostics](live-diagnostics.md)
- [MCP threat model](security/mcp-threat-model.md)
- [Update admission](security/update-admission.md)
- [Current-state manifest](current-state-manifest.json)

Visible native tray owns resident lifecycle. OS-coupled headless child daemon executes Membrane runtime. Hub dashboard is on demand. Stable V1 `hub_inactive` means tray-owned daemon inactive.
