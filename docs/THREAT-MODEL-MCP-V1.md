# Membrane MCP threat model v1

## Boundary

`membrane_context` is a loopback client of `/federate`; `/plan_context` remains provider-facing.
Public tools are limited to context, source read, knowledge proposal, checkpoint save/load, &
feedback. Raw `put`, `get`, `recall`, doctor, schema, & filesystem surfaces are not MCP tools.

## Caller levels

| Level | Allowed actions | Enforcement |
| --- | --- | --- |
| read-only | context, source read, checkpoint load | exact project grant + scope binding |
| write-proposed | knowledge proposal, checkpoint save, feedback | quarantine + provenance + rate limit |
| write-trusted | reviewed promotion only | explicit human/admin disposition |
| admin | enrollment, token rotation, diagnostics | local operator credential |

## Threats & controls

| Threat | Control | Receipt |
| --- | --- | --- |
| Malicious local client requests another repository | Canonical-root registry resolves exact `{repository_id, scope_id, grant_policy}`; unknown/ambiguous roots deny | binding ID + denial reason |
| Client tries raw durable write | No raw write tool; typed `KnowledgeEmission` enters normal admission/quarantine | emission ID + outcome |
| Token copied from config/logs | Token is read from local protected file/env, never echoed; rotate invalidates old token; leak recovery creates replacement, revokes grant, audits event | token generation only, never secret |
| Partial/corrupt registry | Atomic temp-write + replace; parse/schema failure denies every call; no mutable active-project fallback | registry hash + corrupt denial |
| Scope escalation via virtual IDs | Phase 7 is deferred; current calls accept registered canonical roots only | exact scope binding |
| Prompt injection in source/summary | Source remains data; trust classification precedes admission; untrusted text cannot become authority | trust label + disposition |
| Write flood | Per-client proposal/checkpoint limits; bounded payloads; duplicate keys; audit receipts | rate-limit/duplicate outcome |

## Enrollment & recovery

`membrane init <root>` requires explicit local confirmation, writes one atomic project binding, &
returns a receipt. Installer never silently enrolls a root. `membrane token rotate` creates a new
generation then revokes prior grants. Registry restore is from an operator-controlled backup; a
partial restore remains fail-closed.

## Acceptance

Before first external client: JSON-RPC initialization/resource/tool-list tests prove no raw tools;
registry corruption & cross-root requests deny; proposal writes are quarantined/provenanced; token
values are absent from stdout/stderr/receipts.
