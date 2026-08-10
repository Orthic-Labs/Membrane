# Support policy

Ground truth: `release/compatibility.template.json` `supportPolicy`; final compatibility data exists only in sealed release output.

| Field | Value |
|---|---|
| Current line | `0.2.x` |
| Next line | `1.0.0` |
| LTS lines | `0.2.x` |
| Backports | Security fixes only; no feature backports to `0.1.x` |

## What this means

- The `0.2.x` line receives security fixes and is the only actively
  supported line before `1.0.0` ships.
- Feature work lands on the current line going forward, not as backports
  to `0.1.x` or any older line.
- `1.0.0` is the next line; `SECURITY.md` additionally marks `1.0.0`
  release candidates as supported once the 1.0 gate (packets D50–D53)
  passes.

## Reporting and triage

Report vulnerabilities privately per `SECURITY.md` — do not open a public
issue for a security defect. Every report is triaged against the control
mapping in `docs/reference/threat-model.md`.

## Related

- `docs/operations/compatibility.md` — store schema, platform matrix,
  language depth tiers.
- `docs/roadmap.md` — deferred decisions and what changes at 1.0.
