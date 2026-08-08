# Pricing & service boundaries

## Free local baseline

Membrane's correctness baseline is local and is not conditioned on payment in this checkout. It
includes repository-confined context assembly, local persistence, typed
authority and freshness, receipts for admitted and omitted evidence, local
update verification/rollback paths, and export of user-owned records. No account, payment, hosted
service, or remote telemetry is required for those guarantees.

The baseline is the only supported correctness contract today. Availability of
any hosted or paid offering is **undecided**; prices and purchase terms are
**unavailable**. This boundary is not a license or public-availability grant.

## Optional paid capabilities

The following are possible commercial boundaries, not shipped or promised
features:

| Capability | Status | Boundary |
|---|---|---|
| Team sync | **undecided** | Could coordinate opted-in repositories; must not replace local authority or receipts. |
| Fleet administration | **undecided** | Could manage enrollment and policy distribution; local safety remains independent. |
| Central policy management | **undecided** | Could publish policy suggestions; local enforcement and scope authority remain canonical. |
| Paid support | **unavailable** | No response target, SLA, or support channel is published. |
| Managed updates | **undecided** | Could distribute signed releases; local verification, rollback, and update receipts remain required. |
| Optional telemetry | **undecided** | Would require explicit opt-in, documented fields, and an export/off switch; no collection is promised. |

Paid capability must never gate local safety, authority, receipts, updates, or
export. A paid service may add coordination or convenience only; it cannot
turn an unavailable remote dependency into a local correctness guarantee.

## No invented commercial claims

Membrane publishes no price, plan name, usage allowance, uptime target,
retention period, data-collection claim, or availability claim until each is
explicitly decided and documented. See [`support-boundaries.md`](../website/support-boundaries.md)
for the public-facing boundary summary.
