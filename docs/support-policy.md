# Support policy

## Support baseline

Membrane supports the internal, workspace-coupled checkout described in
[`README.md`](../README.md#repository-posture), with Node.js `>=20` and pnpm
11.18.0 ([`package.json`](../package.json)). Client operation support is
exactly the generated matrix in
[`docs/clients/support-matrix.v1.json`](clients/support-matrix.v1.json):
`degraded` means callable but not enforceable; `unsupported` means do not use.

No public SLA, guaranteed response time, or security-support end date is
published (**unavailable**). Platform qualification receipts are required before
claiming installed support ([`docs/evaluation/mbr801-evidence.md`](evaluation/mbr801-evidence.md)).

## Request format

For a defect, include revision, platform, Node version, exact command, expected
and observed result, sanitized logs, and whether data or authorization may be
affected. Remove tokens, private paths, source text, and personal data. Attach a
minimal reproduction only after confirming it contains no secrets.

For a security issue, follow [`SECURITY.md`](../.github/SECURITY.md#report-a-vulnerability)
and use a private maintainer channel; the repository's public contact is
**unavailable**.

## Version and change policy

Keep generated client artifacts synchronized with their registry and contract
tests ([`docs/clients/README.md`](clients/README.md)). Updates are transactional
and receipt-backed ([`docs/update.md`](update.md)); do not infer upgrade safety
from a passing source-only test when installed receipts are missing.

## Commercial boundary

The free/local operational boundary is repository-confined operation with
local safety, authority, typed receipts, update verification/rollback paths, and export. None requires an
account, payment, hosted service, or telemetry. Possible paid team sync,
fleet administration, policy management, managed updates, or optional
telemetry are **undecided**; paid support, prices, and SLAs are **unavailable**.
Any future paid capability is additive and must never gate local safety,
authority, receipts, updates, or export. This is not a license or public-availability grant. See [`pricing.md`](pricing.md).
