# Adapt final plan — repository execution handoff

The accepted design is integrated into the full [Adapt architecture canon](../../../architecture/subsystems/adapt.md) and [atomic capability canon](../../../canon/adapt.md). The detailed audit and donor disposition were supplied separately as `membrane-adapt-improvement-plan-final.md` in the final documentation package. This repository handoff does not reproduce that full audit or claim a new donor review.

Read [implementation status](implementation-status.md) for the actual branch scope and uncompleted work. Source implementation, passing tests, installed host qualification, and measured improvement are distinct evidence levels.

## Work order

1. Freeze actual consumer fixtures and preserve authority/source-binding failures.
2. Route canonical CLI operations through the active resident; allow only explicit stateless offline transforms.
3. Reuse the existing Taste selector through federation, HTTP and CLI; check the final representation and current lifecycle before emission.
4. Keep agent inspection read-only; complete human review and operational health independently.
5. Connect structured host facts to detectors with durable coverage, cursor and replay semantics.
6. Complete reviewed regression cases, bounded external experiments, independent admission and host-authorized guard rollout. A comparison winner or eligible guard is not an activation permission.
7. Qualify real packaged hosts, privacy, lifecycle invalidation and held-out detector behavior before broader rollout.

Preserve explicit Taste authority without imposing an A/B admission gate. Do not infer preference authority from diagnostic evidence, retirement from zero findings, exposure from emission, or prevented failure from guard firing. Pull remains the packet-budget owner and Cortex remains the durable admission owner.
