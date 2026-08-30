# Blueprint Amendment — Archify Absorption Audit

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

> **External evidence reviewed:** current `tt-a1i/archify`.
> **Rule:** absorb mechanisms only. No Archify dependency, renderer, runtime, schema, or viewer enters Blueprint.

## Verdict

Blueprint already owns the harder problem: deterministic repository evidence, graph construction, source identity, freshness, architecture/flow synthesis, semantic diff, and evidence-cited generated views.

Archify contributes a few useful **view-contract and validation ideas**, not a better truth engine.

### Absorb

1. structural architecture view separate from raw graph truth;
2. stable architecture snapshot identity;
3. semantic vs evidence vs projection change classification;
4. separate semantic/evidence/projection digests;
5. strict distinction between route, reach, and impact;
6. machine-actionable local diagnostics;
7. last-known-good publication of derived architecture views;
8. optional purpose-specific completeness profiles.

### Reject

Renderer/viewer/layout/motion/export machinery, agent-authored topology as truth, generic diagram taxonomy, GitHub-specific evidence verification, and any third-party runtime.

## Existing Blueprint foundation

Blueprint already has `ArchitectureFlowViewV1`, which is generation-bound, freshness-aware, bounded, evidence-bearing, and supports truncation/continuation.

Therefore **do not add a generic Archify-like IR**.

Add a sibling structural projection.

## `ArchitectureStructureViewV1`

Purpose:

> What is the smallest bounded structural architecture Blueprint can currently prove for this repository/generation?

Suggested shape:

```text
ArchitectureStructureViewV1
  schemaVersion
  provider
  kind=architecture
  view=structure

  repoId
  generationId
  sourceState
  dirtyFileCount

  synthesis
    providerVersion
    algorithmVersion

  bounds

  components[]
    id
    kind
    role?
    label
    scope?
    evidence[]

  relationships[]
    id
    kind
    sourceId
    targetId
    confidenceTier
    resolved
    evidence[]

  boundaries[]
    id
    kind
    label
    memberIds[]
    evidence[]

  unknowns[]
  omissions[]
  truncated
  continuationCursor?

  digests
    semanticDigest
    evidenceDigest
    projectionDigest
```

This is disposable/rebuildable and never becomes truth storage.

## Three digests

### `semanticDigest`

Covers architecture claims:

- stable component identity/kind/role/scope;
- relationship identity/type/endpoints;
- boundary identity/membership;
- other explicit semantic facts.

Excludes layout, ordering, display prose, renderer state, and styling.

### `evidenceDigest`

Covers evidence backing those claims:

- canonical source/evidence refs;
- spans/hashes;
- provider/confidence;
- generation/fingerprint data required by Blueprint evidence contracts.

This distinguishes:

```text
same architecture, better/different evidence
```

from:

```text
architecture changed
```

### `projectionDigest`

Covers rebuildable view choices:

- layer/order assignment;
- bounded selection;
- projection labels;
- synthesis/projection algorithm version.

Algorithm churn must not masquerade as semantic architecture change.

## `ArchitectureDeltaViewV1`

Compare two generation-bound structure views using stable Blueprint identities.

```text
ArchitectureDeltaViewV1
  repoId
  base { generationId, semanticDigest, evidenceDigest, projectionDigest }
  head { generationId, semanticDigest, evidenceDigest, projectionDigest }
  proofLevel
  completeness
  changes
  summary
  limitations[]
```

Statuses:

```text
component:
  added | removed | semantic_changed | evidence_changed | projection_changed | same

relationship:
  added | removed | topology_changed | semantic_changed
  | evidence_changed | projection_changed | same

boundary:
  added | removed | membership_changed | semantic_changed
  | evidence_changed | projection_changed | same
```

If cross-generation identity is ambiguous, return `ambiguous_identity`; do not guess equivalence.

## Stable identity

Use Blueprint canonical identities, not display labels.

- components -> canonical component/entity IDs;
- relationships -> stable relationship/evidence-path identity;
- boundaries -> deterministic canonical identity;
- aliases/layout IDs -> presentation only.

Do not copy Archify's weaker label-based identity fallback into truth semantics.

## `route != reach != impact`

These must remain separate API/UI semantics.

### Route

Exact directed evidence path between A and B.

### Reach

Nodes reachable through selected proven/authored relation classes.

Reachability is **not automatically** blast radius, breakage, causality, risk, or impact.

### Impact

Blueprint's qualified change-impact result, with its own evidence/rules.

Required invariant:

```text
route != reach != impact
```

## `BlueprintDiagnosticV1`

Absorb Archify's useful local-repair shape:

```text
BlueprintDiagnosticV1
  schemaVersion
  code
  severity
  subject
    kind
    id?
    path?
    field?
  message
  evidence
    generationId
    evidenceRefs[]
    measured?
    expected?
    actual?
  supportedFixes[]
  retryable
```

Example:

```text
code: architecture/missing-evidence
subject: relationship/auth_to_session_store
supportedFixes:
  - remove unsupported relationship
  - supply exact evidence path
  - mark relationship unresolved
```

Goal: agents repair the named defect instead of regenerating/rethinking the entire architecture.

Keep `ArchitectureRuleResultV1` as the canonical rule evaluation record; expose diagnostics as an action-oriented projection/adapter unless a later design proves unification cleaner.

## Last-known-good derived views

Apply the same atomic-publication discipline to generated architecture artifacts:

```text
generation N
 -> build candidate view/artifact
 -> validate schema + evidence + digests
 -> fail: preserve prior valid artifact
 -> pass: atomically publish generation-N artifact
```

Published derived artifacts should be bound to:

```text
repoId
generationId
sourceState
semanticDigest
evidenceDigest
projectionDigest
producerVersion
artifactHash
```

Audit existing output writers before declaring this an implementation gap.

## Evidence-bearing architecture facts

Every displayed architecture claim should explain its evidence.

Especially:

- relationship evidence must support the relationship, not merely prove both endpoint files exist;
- boundary claims require evidence when represented as repository facts;
- synthesized facts expose synthesis provenance;
- unresolved/heuristic relations remain explicitly typed.

Blueprint's current evidence system is stronger than Archify's public-GitHub commit/path verification; do not regress to that model.

## Optional completeness profiles

A later view profile may fail closed when a specific review requires facts that are unknown.

Example:

```text
purpose=deployment_review
requires:
  deployment units
  trust boundaries
  persistence boundaries
  external dependencies
  placement/ownership only where evidenced
unknown_policy=explicit
```

This is P2. Do not let it delay view/delta/digest work.

## Implementation order

### P0
1. audit current architecture-view/artifact producers;
2. add `ArchitectureStructureViewV1`;
3. define stable identity;
4. add three digests;
5. add `ArchitectureDeltaViewV1`;
6. add held-out delta fixtures.

### P1
7. standardize `BlueprintDiagnosticV1`;
8. adapt architecture rule failures to it;
9. document/test `route != reach != impact`;
10. audit last-known-good publication for derived architecture artifacts.

### P2
11. purpose-specific completeness profiles if real review tasks justify them.

## Qualification

Golden cases must separate:

```text
semantic change
!= evidence change
!= projection change
```

Include component/relationship changes, evidence moves, projection-algorithm changes, dirty overlays, source rename with stable identity, ambiguous identity, confidence-tier change, and boundary-membership change.

Diagnostics should be tested for correct subject/evidence and successful **local** repair without unrelated architecture churn.

## Final position

The useful Archify absorption is not prettier diagrams. It is a stronger Blueprint architecture-view contract:

```text
Blueprint graph/evidence
   |
   +--> ArchitectureFlowViewV1       existing
   |
   +--> ArchitectureStructureViewV1  proposed
             |
             +--> semanticDigest
             +--> evidenceDigest
             +--> projectionDigest
             +--> ArchitectureDeltaViewV1
             +--> disposable generated views
```

with:

```text
route != reach != impact
failure -> typed diagnostic -> local repair
```

Zero third-party runtime dependency.
