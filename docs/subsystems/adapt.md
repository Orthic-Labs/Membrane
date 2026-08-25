# Adapt — Governed Behavioral Learning

**Status:** derived subsystem reference · non-normative

**Canonical authority:** `ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md`

**Runtime migration:** `../../migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`

**Implementation paths:** `engine/crates/membrane-transcript/`, `engine/crates/membrane-adapt/`, native `membrane adapt` CLI & Hub scheduler seam

## Purpose

Adapt is Membrane's governed behavioral-learning subsystem.

Adapt has two first-class surfaces:

- **Taste** learns user-backed preferences and behavioral constraints.
- **Insights** learns evidence-backed agent/model/tool failures, gotchas, and waste.

Adapt is not memory. Cortex owns durable admission, lifecycle, storage, retrieval, and delivery.

## Owns

- transcript/event ingestion for behavioral learning;
- evidence canonicalization, provenance, and origin filtering;
- Taste candidate generation and applicability proposals;
- Insights episode detection, issue formation, and recurrence measurement;
- evidence binding, learning audit, rollback metadata, and delivery/effectiveness receipts.

## Does not own

- durable truth or direct durable writes;
- repository truth;
- generic memory creation or document indexing;
- final context admission, budget, representation, or effect authorization.

## Admission boundaries

1. Adapt decides proposal eligibility.
2. Cortex decides durable admission.
3. Membrane planner decides context admission.

Passing one gate grants no authority at another.

## Invariants

1. Taste authority requires qualifying authenticated user evidence.
2. Silent acceptance alone never activates Taste.
3. Insights cannot create Taste authority.
4. Authored policy and explicit current instruction outrank learned preference.
5. Authority class resolves before specificity.
6. Malformed narrowing scope fails closed.
7. Durable Adapt outputs cross typed Cortex admission.
8. Production Adapt is native Rust; Python is release-excluded differential evidence only.

## Completion

- [x] Taste & Insights ship as separate, inspectable surfaces.
- [x] Durable outputs remain evidence-backed, scoped, reversible & receipted.
- [x] Insights detectors pass a portable labelled benchmark before automated effect.
- [x] Native installed Adapt performs mine/review/apply/recall with Python/Node absent.
