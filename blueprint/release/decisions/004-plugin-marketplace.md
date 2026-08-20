# Decision: Third-party plugin marketplace

**Status:** Deferred
**Owner:** Orthic Labs maintainers
**Date:** 2026-08-06
**Packet:** D53

## Decision

The third-party plugin ecosystem is **supported but unmanaged** in 1.0.
Blueprint ships with the plugin and provider contracts (`sdk/providers.mjs`,
`providers/index.mjs`) and accepts third-party plugins, but there is **no
hosted marketplace, no curated catalog, and no automatic discovery** of
community plugins.

## What was measured

- A managed marketplace would normalise discovery (e.g. a single
  `blueprint plugins install foo`), version compatibility, and trust
  boundaries (signing, hash verification, permission manifests).
- Blueprint's plugin trust boundary is intentionally strict (D51): a
  plugin may not escalate `permissions.filesystem/network/process`
  beyond `repo-read/none/none`. A marketplace must enforce this same
  boundary on third-party submissions, which is a non-trivial
  operational commitment.
- The runbook's do-not-absorb list ("third-party plugin ecosystem
  supported but managed-marketplace deferred") makes the decision
  explicit; reversing it is a separate workstream.

## What 1.0 ships

- Plugin contracts (D32, D26) and a documented permission surface.
- The `blueprint.languages.example.toml` shape and a registration flow.
- The MCP plugin-shape extension (no language-table plugins yet).
- A threat-model entry for plugin trust.

## Licensing boundary

Independent plugins may use and redistribute code their authors create against
the Apache-2.0 SDK, schemas, and examples. They must not copy, vendor, modify, or
redistribute proprietary Blueprint core; users obtain core through owner-authorized
channels.

## Reversal conditions

- A review process is in place for the marketplace, including code
  review, automated security tests, and a clear takedown policy.
- A registry contract is shipped that the marketplace can consume
  without changing the plugin trust boundary.
