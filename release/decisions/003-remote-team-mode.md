# Decision: Remote / team mode

**Status:** Deferred
**Owner:** Orthic Labs maintainers
**Date:** 2026-08-06
**Packet:** D53

## Decision

Cortex 1.0 is **local-only**. There is no hosted Cortex service, no
team-mode federation of stores, and no remote tenant boundary. The
federation contracts (D35) exist in code as a typed envelope; they are
**not** enabled by default and not connected to a hosted service.

## What was measured

- A hosted service would unlock cross-repo search across an organisation
  and a shared cache for the slow-path docs. The same contracts serve
  enterprise pilots that run their own federation server.
- A hosted service also creates a privacy boundary: repository content
  is currently treated as data and never leaves the machine, which is
  a hard sell for security-conscious teams. The 1.0 release is the
  right time to make that promise and the wrong time to break it.

## Why deferred, not declined

- The federation envelope (`graph/federation/`, D35) is real code, not
  a sketch. A self-hosted federation server is a supported configuration
  for teams that want it; the deferred surface is the managed offering.
- The CLI `--host` flag, MCP server's allowlist, and SDK's
  `EmbeddedCortexClient` all run on a single machine. None of them is
  an obstacle to a future hosted mode.

## Reversal conditions

- A clear product specification for the hosted service, including the
  data-handling contract, the tenant boundary, and the policy for
  prompt-injected docs that travel through the server.
- A separate 1.x release line is opened for the hosted product, leaving
  the local 1.0 line untouched.
