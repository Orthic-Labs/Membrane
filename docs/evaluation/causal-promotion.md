# Causal learning promotion

MBR-509 permits a retrieval or ranking update only after an immutable controlled comparison closes. Generic feedback remains evidence-only.

## Trust boundary

Independent evidence means a content-free `verdict.recorded` event from trusted loopback `live` or `audit` producer code. Agent-authored feedback, caller-selected `observed_action`, uncited claims, & transform adoption never qualify an update. This is process-bound attribution, not cryptographic protection against a malicious local administrator.

## Preregistration

Before observation, register `LearningExperimentV1` with:

- unique experiment ID;
- exact policy version, activation SHA-256, task class, & closed UTC window;
- minimum observations per control/candidate cohort & integer basis-point margin;
- ordered memory IDs, current content SHA-256 values, & signed score deltas in millionths.

Registration is immutable by canonical request SHA-256. Reusing an ID with different bytes fails. Missing, duplicate, oversized, stale-hash, or unbounded targets fail before persistence.

## Admissible chain

Candidate observations count only when one target has an exact, same-trace chain:

1. `candidate.retrieved` — ranking recommendation;
2. `candidate.admitted` linked through `candidate_of`;
3. `block.delivered` for same artifact;
4. `candidate.used` linked to delivery through `observed_use_of`;
5. `turn.outcome` linked to delivery through `outcome_for`;
6. trusted `verdict.recorded` linked through `verdict_for`.

Every counted trace must also have successful `policy.assigned` & `policy.exposed` events matching policy version, activation hash, cohort, task class, production traffic, & window. Control traces require assignment, exposure, trusted outcome, & verdict. Duplicate verdict traces fail closed.

## Estimator & application

The deterministic effect is:

```text
effect_bps = ((candidate_success * control_count
             - control_success * candidate_count) * 10000)
             / (control_count * candidate_count)
```

Qualification requires both cohort counts to meet preregistered minimum & `effect_bps >= min_effect_bps`. Evidence is capped at 10,000 verdicts and hashed from exact canonical event identities plus request hash.

Qualified application rechecks every target content hash, applies all bounded deltas, marks linked verified feedback qualified, & inserts one immutable receipt in one SQLite transaction. Any changed or missing target rolls back everything. Repeated qualification returns same receipt; rejected comparisons never mutate scores.

## CLI

```sh
cortex --db memory.db learning-register --input experiment.json
cortex --db memory.db learning-qualify EXPERIMENT_ID
cortex --db memory.db learning-receipt EXPERIMENT_ID
cortex --db memory.db backout-schema-v23
```

Backout removes only v23 learning tables plus feedback qualification marker, returning schema v22.
