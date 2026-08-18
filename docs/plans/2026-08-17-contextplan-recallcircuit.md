# Membrane Implementation Guide — Planned Retrieval and Circuit Admission

**Repository:** `Orthic-Labs/Membrane`  
**Pinned base:** `e640aaa77b6d51ddeaf6d1bc825770b6bf7264bd`  
**Tree:** `bca3d94bec357220f34753bfe42dd701a8775ac7`  
**Guide date:** 2026-08-17

## 0. Non-negotiable system boundary

Membrane remains an independent system.

Membrane owns:

- deciding what information a task needs;
- deciding which providers to query;
- provider federation and deadlines;
- authority, freshness and trust gates;
- deduplication;
- token/character budgets;
- admission;
- delivery/rendering;
- context receipts and omissions;
- host hooks/adapters;
- working context and durable memory policy.

Membrane does **not** own:

- Cortex graph construction;
- Cortex entity identity;
- code semantic truth;
- repository graph traversal algorithms;
- Cortex generations;
- a duplicate code graph;
- independent program-analysis semantics.

The integration is:

```text
Membrane
  -> task-shaped structured request to Cortex

Cortex
  -> generation-bound RecallCircuit

Membrane
  -> chooses whether/how much of that evidence reaches the model
```

No store merge. No raw graph merge. No copied Cortex DB.

---

# 1. What changes

Today the federation gateway generally constructs the available provider set and fans it out, while the Rust planner admits candidates after retrieval.

The improved design adds a stage before provider execution:

```text
task
-> ContextPlan
-> selected providers / budgets / Cortex traversal policy
-> provider execution
-> candidate set
-> deterministic admission
-> context packet + receipts
```

The second critical change is how Cortex is consumed:

```text
OLD
Cortex nodes/snippets -> flattened candidates

NEW
Cortex completed evidence path -> one atomic candidate
```

Membrane does not traverse the graph. Cortex does.

---

# 2. Exact P0 file change set

| Action | File | Change |
|---|---|---|
| ADD | `engine/federation/context_plan.py` | Deterministic provider/query plan before fan-out |
| MODIFY | `engine/federation/gateway.py` | Build provider tasks, then execute only the plan-selected subset |
| MODIFY | `engine/federation/providers/cortex.py` | Prefer Cortex RecallCircuit; convert each complete path into one atomic candidate; retain legacy fallback |
| MODIFY | `engine/crates/crypt-core/src/planner.rs` | Recognize `repo_code_circuit`; reward evidence/path completeness without deleting reserved lanes |
| ADD | `engine/federation/test_context_plan.py` | Deterministic planning tests |
| MODIFY | `engine/federation/providers/test_cortex.py` | RecallCircuit parsing, generation pinning and fallback tests |
| MODIFY | Rust planner tests in `planner.rs` | Atomic circuit ranking/admission tests |
| MODIFY | `mcp/context-renderer-lib.cjs` | Optional layout-v2 ordering: constraints front, evidence middle, dirty/live state late |
| MODIFY | `mcp/context-renderer.test.mjs` | Exact byte-order/layout tests |

## Explicit P0 non-changes

Do **not** change in P0:

- `schemas/context-candidate-set.v1.schema.json`;
- canonical `CandidateV1` in `engine/crates/membrane-protocol/src/types.rs`;
- the five core Membrane contract shapes;
- Crypt/vector storage;
- Cortex database or graph;
- working-context schema;
- reserved memory/skill lanes;
- prompt-hook ownership;
- ContextReceipt content-free policy.

A Cortex path can fit inside the existing candidate contract as an **atomic candidate**, so do not create a protocol migration unless evidence requires it.

---

# 3. ADD `engine/federation/context_plan.py`

P0 planning is deterministic and conservative.

It is intentionally **not** an LLM router.

Create:

```python
from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable
import re


_WORD = re.compile(r"[a-z0-9_./:-]+")


@dataclass(frozen=True)
class CortexPlan:
    enabled: bool
    policy_id: str
    max_hops: int
    max_paths: int


@dataclass(frozen=True)
class ContextPlan:
    schema_version: int
    task_class: str
    risk: str
    providers: tuple[str, ...]
    cortex: CortexPlan


def _terms(task: str) -> set[str]:
    return set(_WORD.findall((task or "").lower()))


def _has_any(text: str, needles: Iterable[str]) -> bool:
    lower = text.lower()
    return any(needle in lower for needle in needles)


def build_context_plan(
    task: str,
    *,
    cortex_usable: bool,
    live_usable: bool,
    skills_usable: bool,
) -> ContextPlan:
    text = task or ""
    terms = _terms(text)

    security = _has_any(text, (
        "security", "auth", "authorization", "permission", "credential",
        "secret", "token", "vulnerability", "taint", "trust boundary",
    ))
    migration = _has_any(text, (
        "migration", "migrate", "schema change", "database change",
        "replace storage", "move from", "upgrade protocol",
    ))
    architecture = _has_any(text, (
        "architecture", "architect", "boundary", "component",
        "dependency", "coupling", "design", "interface",
    ))
    impact = _has_any(text, (
        "what breaks", "blast radius", "impact", "depends on",
        "dependency", "callers", "consumers",
    ))
    debug = _has_any(text, (
        "bug", "debug", "failing", "failure", "crash", "regression",
        "exception", "incorrect", "root cause",
    ))
    docs = _has_any(text, (
        "readme", "docs", "documentation", "document",
    ))
    local_edit = (
        _has_any(text, ("rename", "typo", "format", "comment", "small edit"))
        and not (security or migration or architecture or impact or debug)
    )

    if security:
        task_class, risk = "security", "high"
        policy, hops, paths = "impact.reverse", 5, 24
    elif migration:
        task_class, risk = "migration", "high"
        policy, hops, paths = "impact.reverse", 5, 24
    elif architecture:
        task_class, risk = "architecture", "high"
        policy, hops, paths = "architecture.boundary", 4, 24
    elif impact:
        task_class, risk = "impact", "medium"
        policy, hops, paths = "impact.reverse", 4, 20
    elif debug:
        task_class, risk = "debug", "medium"
        policy, hops, paths = "dependency.forward", 4, 20
    elif local_edit:
        task_class, risk = "local_edit", "low"
        policy, hops, paths = "explore.both", 2, 8
    elif docs:
        task_class, risk = "docs", "low"
        policy, hops, paths = "explore.both", 2, 8
    else:
        task_class, risk = "general", "medium"
        policy, hops, paths = "explore.both", 3, 16

    providers: list[str] = []

    # Always-admissible identity/policy lanes.
    providers.extend(("rules", "anchors", "git"))

    if live_usable:
        providers.append("live")

    if cortex_usable and task_class != "docs":
        providers.append("cortex")

    if task_class in {"security", "migration", "architecture"}:
        providers.extend(("audit", "architect"))
        if skills_usable:
            providers.append("skills")
        providers.append("crypt")

    elif task_class == "debug":
        providers.append("audit")
        if skills_usable:
            providers.append("skills")

    elif task_class == "impact":
        if skills_usable:
            providers.append("skills")

    elif task_class == "general":
        # Conservative default: preserve today's broad coverage for ambiguous
        # work until retrieval evaluation proves a narrower plan is safe.
        providers.extend(("audit", "architect", "crypt"))
        if skills_usable:
            providers.append("skills")

    # Low-risk local edits and docs deliberately skip expensive advisory/history
    # lanes unless the task text actually classifies into a stronger class.
    providers = list(dict.fromkeys(providers))

    return ContextPlan(
        schema_version=1,
        task_class=task_class,
        risk=risk,
        providers=tuple(providers),
        cortex=CortexPlan(
            enabled=cortex_usable and "cortex" in providers,
            policy_id=policy,
            max_hops=hops,
            max_paths=paths,
        ),
    )
```

## Why P0 is conservative

Do not "optimize" by skipping providers on ambiguous requests.

P0 narrows only obvious low-risk cases.

For `general`, preserve today's broad provider coverage.

This makes the rollout falsifiable and reversible.

---

# 4. MODIFY `engine/federation/gateway.py`

## 4.1 Import the planner

Add:

```python
from federation.context_plan import build_context_plan
```

## 4.2 Build all available task factories exactly once

Keep the existing provider adapters and freshness checks.

Replace the current direct `tasks = [...]` construction with:

```python
all_tasks: dict[str, Any] = {
    "audit": lambda: _adapter("audit", audit.produce, repo_root, task),
    "architect": lambda: _adapter("architect", architect.produce, repo_root, task),
    "crypt": lambda: _adapter(
        "crypt",
        crypt.produce_with_observability,
        repo_root,
        task,
        scope_grant_id,
        scope_descriptor,
    ),
    "git": lambda: _adapter("git", git_provider.produce, repo_root),
    "rules": lambda: _adapter("rules", rules.produce, repo_root, task, client),
    "anchors": lambda: _adapter(
        "anchors",
        anchors.produce,
        repo_root,
        explicit_anchors,
        task,
    ),
}
```

Then conditionally add live/skills/cortex:

```python
cortex_usable = bool(cortex_state.get("usable"))
live_usable = bool(overlay_state.get("usable"))
skills_usable = bool(skills_state.get("usable"))

plan = build_context_plan(
    task,
    cortex_usable=cortex_usable,
    live_usable=live_usable,
    skills_usable=skills_usable,
)

if cortex_usable:
    all_tasks["cortex"] = lambda: _adapter(
        "cortex",
        cortex.produce_with_observability,
        repo_root,
        task,
        max_tokens,
        expected_generation=expected_cortex_generation,
        policy_id=plan.cortex.policy_id,
        max_hops=plan.cortex.max_hops,
        max_paths=plan.cortex.max_paths,
    )

if live_usable:
    all_tasks["live"] = lambda: _adapter(
        "live",
        live.produce,
        repo_root,
        base_commit=freshness.get("baseCommit"),
        overlay_digest=freshness.get("overlayDigest"),
        overlay_entries=verdict.get("overlayEntries") or [],
        prompt_fast=True,
    )

if skills_usable:
    all_tasks["skills"] = lambda: _adapter(
        "skills",
        skills.produce,
        repo_root,
        task,
        scope_grant_id,
    )

tasks = [
    (name, all_tasks[name])
    for name in plan.providers
    if name in all_tasks
]
```

## 4.3 Keep current bounded executor in P0

Do **not** rewrite `_collect_tasks_bounded()` in the same patch.

It already:

- enforces one absolute deadline;
- isolates provider failures;
- gives Cortex special scheduling because it is a structural dependency;
- emits typed timeout warnings.

Changing routing and concurrency semantics simultaneously would make regressions harder to attribute.

P1 may replace the Cortex special-case with generic stages after ContextPlan is qualified.

## 4.4 Do not put raw task text into receipts

If ContextPlan observability is emitted, expose only:

```json
{
  "schemaVersion": 1,
  "taskClass": "impact",
  "risk": "medium",
  "providers": ["rules", "anchors", "git", "live", "cortex", "skills"],
  "cortexPolicyId": "impact.reverse"
}
```

Do not duplicate `task` content into telemetry/receipt surfaces.

---

# 5. MODIFY `engine/federation/providers/cortex.py`

The provider currently normalizes Cortex's flat candidates. Change it to prefer `RecallCircuitV1`.

## 5.1 Function signature

Extend the public provider path:

```python
def produce_with_observability(
    repo_root: Path,
    task: str,
    max_tokens: int,
    *,
    expected_generation: str,
    policy_id: str = "explore.both",
    max_hops: int = 3,
    max_paths: int = 16,
):
```

Apply the same optional args through the internal `_produce(...)` path.

## 5.2 Cache key

Current cache identity already includes task/cap/generation.

Add:

```text
policy_id
max_hops
max_paths
```

A result produced under `explore.both` must never satisfy an `impact.reverse` cache lookup.

## 5.3 Prefer the lean RecallCircuit script

Derive:

```python
recall_cli = Path(cortex_cli).with_name("cortex-recall.mjs")
```

If it exists, invoke:

```python
cmd = [
    node,
    str(recall_cli),
    "--root", str(repo_root),
    "--task", task,
    "--policy", policy_id,
    "--max-hops", str(max_hops),
    "--max-paths", str(max_paths),
    "--expected-generation", expected_generation,
]
```

If it does not exist, use the existing `cortex-candidates.mjs` flow unchanged.

This gives version-skew compatibility.

## 5.4 Validate before using

Require:

```python
document["schemaVersion"] == 1
document["generationId"] == expected_generation
isinstance(document["paths"], list)
isinstance(document["nodes"], list)
isinstance(document["edges"], list)
```

On mismatch:

- return no Cortex candidate;
- emit a typed warning;
- do not silently reinterpret it as legacy candidate output.

## 5.5 Convert one path into one atomic candidate

Add helpers:

```python
def _sha256_json(value: Any) -> str:
    encoded = json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _node_label(node: dict[str, Any]) -> str:
    return str(
        node.get("qualifiedName")
        or node.get("name")
        or node.get("path")
        or node.get("id")
        or "unknown"
    )


def _render_circuit_path(
    path: dict[str, Any],
    nodes_by_id: dict[str, dict[str, Any]],
    edges_by_id: dict[str, dict[str, Any]],
) -> str:
    node_ids = list(path.get("nodeIds") or [])
    edge_ids = list(path.get("edgeIds") or [])
    if not node_ids:
        return ""

    parts = [_node_label(nodes_by_id.get(node_ids[0], {}))]
    for index, edge_id in enumerate(edge_ids):
        edge = edges_by_id.get(edge_id, {})
        kind = str(edge.get("kind") or "RELATES_TO")
        target = node_ids[index + 1] if index + 1 < len(node_ids) else "unknown"
        parts.append(f"--[{kind}]--> {_node_label(nodes_by_id.get(target, {}))}")

    evidence_refs: list[str] = []
    for edge_id in edge_ids:
        for ev in edges_by_id.get(edge_id, {}).get("evidence") or []:
            p = ev.get("path")
            if not p:
                continue
            start = ev.get("startLine")
            end = ev.get("endLine")
            ref = str(p)
            if start:
                ref += f":{start}"
                if end and end != start:
                    ref += f"-{end}"
            evidence_refs.append(ref)

    text = " ".join(parts)
    if evidence_refs:
        text += "\nEvidence: " + ", ".join(dict.fromkeys(evidence_refs))
    return text
```

Then:

```python
def _circuit_candidates(
    document: dict[str, Any],
    *,
    expected_generation: str,
) -> list[dict[str, Any]]:
    nodes_by_id = {
        str(node.get("id")): node
        for node in document.get("nodes") or []
        if node.get("id")
    }
    edges_by_id = {
        str(edge.get("id")): edge
        for edge in document.get("edges") or []
        if edge.get("id")
    }

    candidates: list[dict[str, Any]] = []

    for path in document.get("paths") or []:
        if not path.get("complete"):
            continue

        text = _render_circuit_path(path, nodes_by_id, edges_by_id)
        if not text:
            continue

        seed_id = str(path.get("seedId") or "")
        terminal_id = str(path.get("terminalId") or "")
        path_id = str(path.get("id") or "")
        descriptor = {
            "generationId": expected_generation,
            "pathId": path_id,
            "seedId": seed_id,
            "terminalId": terminal_id,
            "nodeIds": list(path.get("nodeIds") or []),
            "edgeIds": list(path.get("edgeIds") or []),
        }

        score = float(path.get("score") or 0.0)
        evidence_coverage = float(path.get("evidenceCoverage") or 0.0)

        candidates.append({
            "id": f"cortex-circuit:{document['circuitId']}:{path_id}",
            "layer": 3,
            "provider": "cortex",
            "sourceKind": "repo_code_circuit",
            "sourceRef": (
                f"cortex://circuit/{document['circuitId']}/{path_id}"
            ),
            "sourceHash": _sha256_json(descriptor),
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            "providerScore": max(0.0, min(1.0, score)),
            "scoreComponents": {
                "path_complete": 1.0,
                "evidence_complete": 1.0
                    if path.get("evidenceComplete") else 0.0,
                "evidence_coverage": max(
                    0.0, min(1.0, evidence_coverage)
                ),
                "hop_efficiency": 1.0 / (
                    1.0 + len(path.get("edgeIds") or [])
                ),
            },
            "freshnessClass": "current",
            "estimatedTokens": max(
                1,
                (len(text.encode("utf-8")) + 3) // 4,
            ),
            "protected": False,
            "exact": bool(
                path.get("complete")
                and path.get("evidenceComplete")
            ),
            "recoverable": True,
            "resolver": (
                f"cortex graph path {seed_id} {terminal_id}"
            ),
            "text": text,
        })

    return candidates
```

## Why the whole path is one candidate

Do not let Membrane admit:

```text
A
B
C
```

independently when the evidence is:

```text
A -> B -> C
```

The path is the semantic unit.

This avoids top-k admission splitting a required chain.

## 5.6 Empty circuit behavior

If Cortex returns:

```json
{
  "paths": [],
  "unresolved": [{"reason": "no_relevant_seed"}]
}
```

emit no Cortex candidate.

Do not convert the unresolved state into generic repository text.

Preserve the current loud abstention/warning semantics.

---

# 6. MODIFY `engine/crates/crypt-core/src/planner.rs`

P0 does **not** replace the planner.

It improves how complete graph evidence is treated.

## 6.1 Add circuit source-kind priority

Current `kind_priority()` starts:

```rust
"repo_code" | "repo_code_overlay" => 0,
```

Change to:

```rust
"repo_code" | "repo_code_overlay" | "repo_code_circuit" => 0,
```

## 6.2 Add deterministic circuit-quality helpers

Add near `freshness_component()` / `kind_priority()`:

```rust
fn score_component(cand: &CandidateV1, key: &str) -> f64 {
    cand.score_components
        .get(key)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn circuit_quality(cand: &CandidateV1) -> f64 {
    if cand.source_kind != "repo_code_circuit" {
        return 0.0;
    }
    let complete = score_component(cand, "path_complete");
    let evidence = score_component(cand, "evidence_coverage");
    let hop_efficiency = score_component(cand, "hop_efficiency");

    (complete * 0.45 + evidence * 0.45 + hop_efficiency * 0.10)
        .clamp(0.0, 1.0)
}
```

## 6.3 Extend the deterministic sort

Current order is approximately:

```text
protected
provider_score
freshness
kind_priority
exact
id
```

Change to:

```rust
deduped.sort_by(|a, b| {
    let af = freshness_component(a);
    let bf = freshness_component(b);
    let aq = circuit_quality(a);
    let bq = circuit_quality(b);
    let ak = kind_priority(a);
    let bk = kind_priority(b);

    b.protected
        .cmp(&a.protected)
        .then_with(|| {
            bq.partial_cmp(&aq)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            b.provider_score
                .partial_cmp(&a.provider_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            bf.partial_cmp(&af)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then(ak.cmp(&bk))
        .then(b.exact.cmp(&a.exact))
        .then(a.id.cmp(&b.id))
});
```

## 6.4 Do not delete reserved lanes

Keep:

```rust
const RESERVED_LANES: &[(&str, usize)] =
    &[("memory", 800), ("skill", 300)];
```

and the Git identity / repo-code protections.

Reason: current provider scores are explicitly not calibrated as one cross-provider probability.

A new paper does not justify deleting a working anti-starvation policy.

P1 can replace lanes only after calibration data shows a better global policy.

## 6.5 Add tests

Add tests to the existing Rust module:

### Complete circuit beats incomplete circuit at equal lane score

```rust
#[test]
fn complete_cortex_circuit_beats_incomplete_peer() {
    let mut complete = candidate(
        "circuit:complete",
        "repo_code_circuit",
        80,
        0.8,
        false,
    );
    complete.provider = Some("cortex".into());
    complete.score_components.insert("path_complete".into(), 1.0);
    complete.score_components.insert("evidence_coverage".into(), 1.0);
    complete.score_components.insert("hop_efficiency".into(), 0.5);

    let mut incomplete = candidate(
        "circuit:partial",
        "repo_code_circuit",
        80,
        0.8,
        false,
    );
    incomplete.provider = Some("cortex".into());
    incomplete.score_components.insert("path_complete".into(), 0.0);
    incomplete.score_components.insert("evidence_coverage".into(), 0.5);

    let out = plan(&empty_planner_input(
        vec![incomplete, complete],
    )).unwrap();

    assert_eq!(
        out.packet.blocks.first().unwrap().id,
        "circuit:complete"
    );
}
```

### Circuit stays atomic

Assert one path candidate creates exactly one admitted block; planner never splits node/edge components because they are not separate candidates.

### Reserved lanes remain

Existing memory/skill lane tests must continue passing.

---

# 7. MODIFY `mcp/context-renderer-lib.cjs` — layout v2

The renderer currently sorts blocks primarily by descending priority.

That is deterministic, but it ignores context-position effects.

Do not introduce a protocol field in P0. Use existing provider/source information.

## 7.1 Add placement helper

```js
function contextPlacementRank(block) {
  const provider = String(block?.provider || "");
  const sourceKind = String(block?.sourceKind || block?.source_kind || "");

  // Front: hard constraints / policy / pinned anchors.
  if (
    provider === "rules" ||
    sourceKind === "rule" ||
    sourceKind === "policy" ||
    block?.protected === true
  ) {
    return 0;
  }

  // Late: dirty/live state should sit near the active task boundary.
  if (
    provider === "live" ||
    sourceKind === "repo_code_overlay" ||
    sourceKind === "working_context"
  ) {
    return 2;
  }

  // Middle: evidence, circuits, docs, memory, skills, audit, architecture.
  return 1;
}
```

## 7.2 Put behind rollout flag first

Inside `finalize()` change the order creation to:

```js
const layoutV2 =
  process.env.MEMBRANE_CONTEXT_LAYOUT_V2 === "1";

const order = blocks
  .map((block, index) => ({ block, index }))
  .sort((left, right) => {
    if (layoutV2) {
      const placement =
        contextPlacementRank(left.block) -
        contextPlacementRank(right.block);
      if (placement !== 0) return placement;
    }

    return (
      Number(right.block.priority || 0) -
        Number(left.block.priority || 0) ||
      left.index - right.index
    );
  });
```

## 7.3 Required tests

`mcp/context-renderer.test.mjs`:

- rules precede evidence under layout v2;
- Cortex circuit is in evidence middle;
- live/dirty overlay follows ordinary evidence;
- same packet produces byte-identical output across repeated runs;
- layout v1 remains unchanged when flag off;
- char-budget accounting remains exact;
- renderer/Forge parity test remains exact.

Do not graduate the flag until answer-quality evaluation shows non-regression.

---

# 8. Do not change protocol v1 in P0

`ContextCandidateSetV1.candidate` is closed (`additionalProperties:false`) and the Rust type is `deny_unknown_fields`.

Do not casually add:

```text
groupId
workingSetClass
placementClass
pathIds
```

to only one language/schema.

For P0, encode the path as one atomic candidate using existing fields:

- `id`
- `sourceKind`
- `sourceRef`
- `sourceHash`
- `providerScore`
- numeric `scoreComponents`
- `resolver`
- `text`

This gets the system benefit without contract churn.

---

# 9. P1 — Retrieval sufficiency evaluator

After static ContextPlan ships, add:

- `engine/federation/retrieval_evaluator.py`
- tests.

Its job is **not** to answer the task.

It decides whether another retrieval stage is justified.

Output:

```json
{
  "schemaVersion": 1,
  "verdict": "sufficient",
  "reasons": [
    "fresh_structural_path",
    "evidence_complete"
  ],
  "missing": []
}
```

Allowed verdicts:

```text
sufficient
insufficient
ambiguous
contradictory
stale
unsafe
provider_failure
```

P1 staged flow:

```text
Stage 0: identity/rules/live
Stage 1: Cortex structural recall
evaluate
Stage 2: audit/architect/memory/skills only if justified
evaluate
Stage 3: expensive semantic escalation only if still justified
```

## Stop condition

Do not retrieve more merely because budget remains.

Retrieve more when expected decision value is positive.

P0 can approximate this with deterministic rules; do not add another LLM call to decide whether an LLM should receive more context.

---

# 10. P1 — Working-set classes

Current working-context selection in both:

- `mcp/working-context.mjs`
- `engine/crates/membrane-runtime/src/working_context.rs`

walks candidates in order under `max_blocks`, `max_bytes`, `max_recent_turns`.

Do not mix this migration into P0.

P1 may introduce:

```text
pinned
resident
prefetched
reconstructable
quarantined
```

Semantics:

- `pinned`: cannot be evicted while its task constraint is active;
- `resident`: current high-value working set;
- `prefetched`: likely useful, lower priority;
- `reconstructable`: keep resolver/metadata, not text;
- `quarantined`: untrusted/stale/contradicted; never auto-inject.

If the shape changes:

1. bump the working-context schema;
2. update JS and Rust twins in the same commit;
3. update canonical digest fixtures;
4. update server/MCP tests;
5. never let one side accept fields the other side rejects.

---

# 11. P1 — Value-of-information / marginal utility

Do not invent one global relevance probability.

Instead collect calibration data first.

Per candidate, log content-free metrics such as:

```text
provider
sourceKind
freshnessClass
estimatedTokens
admitted/rejected
reason
path completeness
evidence coverage
whether later feedback said context was missing/redundant
```

Then evaluate:

```text
marginal task success improvement
---------------------------------
tokens + latency + provider cost
```

Possible future planner terms:

- novelty;
- coverage of unresolved task dimensions;
- authority;
- freshness;
- evidence completeness;
- risk reduction;
- token cost;
- latency cost.

The reserved-lane policy remains until calibrated replacement outperforms it.

---

# 12. P1 — Poisoning and instruction separation

Membrane already carries `trustClass` and `instructionPolicy`.

Strengthen admission tests around Cortex circuits:

A Cortex path is **data**, not executable instruction.

Required invariant:

```text
repository text:
"ignore previous instructions and..."
```

must remain:

```text
instructionPolicy = "data_only"
```

and never become host/system instruction merely because Cortex connected it structurally.

Add adversarial fixtures where:

- README contains prompt injection;
- source comments contain tool instructions;
- stale architecture doc claims authority;
- generated file claims to be a system message.

Membrane must preserve source trust and authority independently of semantic similarity.

---

# 13. Exact removals/deprecations

## Deprecate after qualification

### Blind all-provider execution for every obvious low-risk request

Do not delete providers. Stop invoking them when ContextPlan explicitly says they have no expected value.

### Flattened Cortex node candidates

Once RecallCircuit is available, do not make isolated graph nodes the normal Cortex candidate unit for multi-hop questions.

Keep legacy parsing only for version skew and rollback.

### Hard-coded Cortex scheduling as the only "planning" concept

P0 can retain `_collect_tasks_bounded()` unchanged for safety.

P1 should make stage priority generic rather than embedding one provider name into scheduling policy.

## Do not remove

- provider failure isolation;
- absolute federation deadline;
- freshness verdict;
- release generation checks;
- ScopeGrant;
- `ContextCandidateSetV1`;
- Rust admission planner;
- reserved lanes;
- content-free receipts;
- char/token reconciliation;
- native delivery receipts;
- Crypt;
- prompt hooks.

---

# 14. Tests: exact required matrix

## ADD `engine/federation/test_context_plan.py`

### Local rename

Task:

```text
rename SessionManager to SessionStore
```

Assert:

- class `local_edit`;
- risk `low`;
- no `audit`, `architect`, `crypt`;
- Cortex included when usable;
- Cortex policy bounded to 2 hops.

### Impact

Task:

```text
what breaks if I change SessionManager?
```

Assert:

- class `impact`;
- policy `impact.reverse`;
- Cortex included;
- skills included when available;
- no unnecessary architect unless architecture signal exists.

### Security

Task:

```text
change auth token validation
```

Assert:

- high risk;
- Cortex + audit + architect + skills + crypt;
- policy `impact.reverse`.

### General ambiguity

Assert broad current provider set is preserved.

### Provider unavailable

If Cortex is unusable:

- plan does not include it;
- caller still receives non-Cortex providers;
- no fake graph candidate is created.

---

## MODIFY `engine/federation/providers/test_cortex.py`

Required new cases:

1. reads RecallCircuit v1;
2. rejects generation mismatch;
3. falls back to legacy candidate script if recall script absent;
4. one complete path => one candidate;
5. path source hash is deterministic;
6. cache key differs by policy/hops/paths;
7. no-seed circuit => zero candidates + typed abstention;
8. prompt-injection text in evidence remains `data_only`;
9. incomplete path is not emitted as exact;
10. evidence coverage reaches planner `scoreComponents`.

---

## Rust planner tests

Required:

- `repo_code_circuit` has repo-code priority;
- complete/evidenced circuit beats incomplete peer at equal lane score;
- source-hash dedup still works;
- circuit remains one block;
- memory and skills reserved lanes still function;
- global token ceiling still holds.

---

## Renderer tests

Required:

- flag off == current byte output;
- flag on is deterministic;
- rules front;
- ordinary evidence middle;
- live/dirty state late;
- packet char cap still enforced;
- budget reconciliation remains balanced;
- Forge and Membrane renderer remain byte-identical.

---

# 15. Benchmarks and qualification gates

Do not optimize only latency.

Measure:

## Retrieval-plan metrics

- providers invoked per task class;
- provider timeout rate;
- provider failure rate;
- candidate count before admission;
- selected tokens;
- delivered tokens;
- missing-context feedback rate.

## Cortex-circuit metrics

- path completeness;
- evidence coverage;
- average hops;
- path candidate token cost;
- percentage of multi-hop tasks solved without model-driven search.

## End-to-end

For each task category:

```text
legacy federation
vs
ContextPlan + RecallCircuit
```

Measure:

- task correctness;
- tool calls after context delivery;
- retrieval wall time;
- total tokens;
- context omissions;
- stale/incorrect evidence;
- model tier sensitivity.

### Graduation criteria

Do not pick arbitrary "5% faster" gates.

P0 graduates when:

1. no correctness regression on current qualification fixtures;
2. fewer unnecessary provider calls on low-risk tasks;
3. graph multi-hop fixtures need fewer model tool calls;
4. no rise in stale/poisoned context admission;
5. receipt/budget invariants remain exact;
6. rollback path is tested.

---

# 16. Rollout sequence

## Commit M1 — ContextPlan in shadow

- add `context_plan.py`;
- call it;
- record selected provider names in test/shadow diagnostics;
- still execute current full set in production.

Purpose: compare "would run" vs "did run."

## Commit M2 — activate planning for low-risk classes

Only narrow:

- local edit;
- docs.

Keep `general` broad.

## Commit M3 — Cortex RecallCircuit consumption

- prefer new lean script;
- path -> atomic candidate;
- legacy fallback retained.

## Commit M4 — planner circuit quality

- add `repo_code_circuit`;
- add deterministic quality tie-break;
- keep reserved lanes.

## Commit M5 — renderer layout v2 shadow

- feature flag only;
- qualification A/B.

## Commit M6 — broader task classes

After evidence, allow planner narrowing for:

- impact;
- debug;
- architecture;
- migration;
- security.

Do not start here.

---

# 17. Rollback

Every P0 change has a direct rollback.

## ContextPlan failure

Set planning off and execute current full provider set.

## RecallCircuit failure/version skew

`cortex.py` falls back to `cortex-candidates.mjs`.

## Planner regression

Remove `repo_code_circuit` tie-break; path candidate still fits existing v1 schema.

## Renderer regression

Unset:

```text
MEMBRANE_CONTEXT_LAYOUT_V2
```

No stored-data migration is involved.

---

# 18. Definition of Done

P0 is done only when all are true:

- [ ] Membrane remains a separate system.
- [ ] No Cortex graph/store logic is copied into Membrane.
- [ ] ContextPlan runs before provider execution.
- [ ] Low-risk requests can skip providers with no expected value.
- [ ] Ambiguous general requests preserve broad coverage.
- [ ] Cortex RecallCircuit is preferred when available.
- [ ] Legacy Cortex candidates remain a rollback/version-skew fallback.
- [ ] Each Cortex path is atomic in admission.
- [ ] Generation mismatch fails closed.
- [ ] No-seed circuit creates no fake context.
- [ ] Repository content remains `data_only`.
- [ ] Reserved memory/skill lanes remain intact.
- [ ] Existing token and char ceilings remain exact.
- [ ] Receipts remain content-free.
- [ ] Provider failure isolation remains intact.
- [ ] Layout v2 is feature-flagged until qualified.
- [ ] No v1 protocol fields are added ad hoc.
- [ ] JS/Rust parity tests pass.
- [ ] `pnpm test` passes.
- [ ] `cargo test --workspace --features fastembed` passes.
- [ ] End-to-end qualification shows correctness non-regression.

---

# 19. Architectural end state

```text
TASK
  |
  v
Membrane ContextPlan
  |
  +--> choose providers
  +--> choose Cortex policy
  +--> allocate deadlines
  |
  v
PROVIDERS
  |
  +--> Cortex computes RecallCircuit
  +--> rules
  +--> live state
  +--> Git
  +--> memory
  +--> skills
  +--> audit/architect when justified
  |
  v
ContextCandidateSet
  |
  v
deterministic admission
  |
  +--> trust
  +--> authority/freshness
  +--> dedup
  +--> complete-path quality
  +--> reserved lanes
  +--> global budget
  |
  v
ContextPacket + ContextReceipt
  |
  v
renderer / host delivery
  |
  v
MODEL
```

The important change is not "retrieve more intelligently."

It is:

**Membrane decides which computations are worth running, while Cortex supplies already-computed structural evidence rather than forcing the model to rediscover graph relationships.**
