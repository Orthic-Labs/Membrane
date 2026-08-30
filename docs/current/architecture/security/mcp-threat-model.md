# Membrane MCP threat model

**Status:** Current security architecture
**Surface:** Native stdio MCP & authenticated Streamable-HTTP MCP
**Runtime:** Stateless clients connect to active tray-owned daemon; closing Hub dashboard has no effect. `hub_inactive` remains V1 compatibility token for daemon inactivity.

## Boundary

`membrane_context` reaches canonical `/federate` behavior. All 17 public tools use same grant, authorization, scope, deadline, receipt, & typed-error boundaries:

- `membrane_blueprint`;
- `membrane_checkpoint_load`, `membrane_checkpoint_save`;
- `membrane_context`;
- `membrane_diagnostic_baseline`, `membrane_diagnostic_capabilities`, `membrane_diagnostic_fence`, `membrane_diagnostic_mutation`, `membrane_diagnostic_provider`, `membrane_diagnostic_snapshot`, `membrane_diagnostic_workspace`;
- `membrane_feedback`, `membrane_knowledge_propose`;
- `membrane_scratchpad`, `membrane_source_read`, `membrane_temporal_fact`, `membrane_working_context`.

Raw database, arbitrary filesystem, token, enrollment, daemon-start, doctor, schema-mutation, & direct durable-write operations are not MCP tools. MCP & CLI never start or register residency.

## Caller levels

| Level | Allowed actions | Enforcement |
|---|---|---|
| Read | Context, Blueprint, source, temporal, checkpoint/working-context load, diagnostics read | Exact installation + grant + repository/scope/root binding |
| Proposed write | Knowledge proposal, feedback, checkpoint/working-context/scratchpad updates, diagnostic epochs/baselines | Typed schema + bounded payload + provenance + authorization + lifecycle receipt |
| Host enforcement | Diagnostic fence/mutation/provider controls | Authenticated host identity + exact workspace epoch/snapshot/policy binding |
| Operator | Enrollment, token rotation, repair, update, service lifecycle | Local operator surface outside MCP |

No caller level grants semantic authority. Repository/model text remains data. Proposed writes pass owning subsystem admission; host enforcement cannot rewrite planner policy.

## Threats & controls

| Threat | Control | Required evidence |
|---|---|---|
| Cross-root request | Canonical root registry resolves exact installation, repository, scope, grant, caller, & target; unknown/ambiguous/cross-root denies | Binding identity + typed denial |
| Client starts hidden runtime | MCP/CLI are stateless clients; inactive tray-owned daemon returns `membrane_unavailable { hub_inactive }` & spawns nothing | Process/lifecycle test |
| Origin/host/token abuse over HTTP | Loopback bind, authenticated Streamable HTTP, strict origin/host/token checks, rotation, no credential reflection | Negative transport tests |
| Raw durable write | No raw write tool; `KnowledgeEmission` enters Cortex pending/quarantine/admission path | Emission ID + disposition |
| Unauthorized diagnostic mutation/restart | Exact scope grant, caller class, target, workspace epoch, provider identity, & mutation lifecycle checks | Mutation/provider receipt |
| Stale edit-fence clearance | Fence binds exact epoch, source hashes, diagnostic snapshot, generation, policy; mismatch supersedes clearance | Fence decision + digests |
| Prompt injection/self-authorization | Source, docs, memories, model output, & summaries cannot grant authority; trust/influence checks precede admission | Trust label + disposition |
| Partial/corrupt registry or store | Atomic publish, schema/integrity validation, last-known-good where owned, fail-closed active reads | Generation/store identity + typed failure |
| Token disclosure | Token stays protected native memory/file boundary, never stdout/stderr/webview/receipt; rotation revokes old generation | Content-free token generation receipt |
| Flood/resource exhaustion | Payload, item, graph, result, token, provider, retry, & absolute-deadline caps with cancellation | Budget/deadline/omission receipt |
| Scratchpad/checkpoint laundering | Non-authoritative lifecycle, scope, expiry, tamper evidence, explicit promotion through normal admission only | Scope/lifecycle receipt |
| Blueprint evidence confusion | Generation/freshness/path completeness stay explicit; Membrane never opens Blueprint SQLite | Result envelope + omissions |
| False healthy/complete result | Unknown, timeout, stale, inaccessible, uninstrumented, budget drop, & omitted sources remain typed | Packet/snapshot/fence receipt |

## Enrollment & recovery

Enrollment requires explicit local operator action outside MCP & writes one atomic project binding. Installer never silently enrolls roots. Token rotation creates new generation before revoking old grants. Corrupt or partial registry/store state denies affected calls until operator repair. Support bundles are bounded, content-free where required, hash-manifested, & authorized.

## Acceptance

Before external-client qualification:

1. tool discovery exposes exactly current 17-tool contract for selected toolset;
2. raw database/filesystem/token/enrollment/daemon-start surfaces remain absent;
3. cross-root, expired/revoked grant, unsafe origin/host/token, corrupt registry, stale generation, & stale diagnostic epoch deny with typed errors;
4. proposal writes retain provenance & admission/quarantine semantics;
5. inactive tray-owned daemon spawns nothing & returns `hub_inactive` compatibility reason;
6. tokens & sensitive payloads remain absent from logs, webview data, errors, receipts, & support bundles;
7. stdio & HTTP projections preserve same application behavior, authorization, typed errors, & receipts.
