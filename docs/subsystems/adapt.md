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

1. Taste authority requires an explicit user-selected transcript, exact source hash/span binding, external-user attribution, & required review.
2. Silent acceptance alone never activates Taste.
3. Insights cannot create Taste authority.
4. Authored policy and explicit current instruction outrank learned preference.
5. Authority class resolves before specificity.
6. Malformed narrowing scope fails closed.
7. Durable Adapt outputs cross typed Cortex admission.
8. Native Rust is the production owner. Legacy Python is migration/differential
   material whose exclusion from the exact release artifact remains an open N10 gate.

## Completion

- [x] Taste & Insights ship as separate, inspectable surfaces.
- [x] Durable outputs remain evidence-backed, scoped, reversible & receipted.
- [x] Insights detectors have a portable labelled benchmark and committed measured
  results with documented gaps; automated effect remains blocked pending its gates.
- [x] Taste passes its predeclared synthetic conformance thresholds (extraction
  precision/recall 0.9667/1.0; admission 0.9524/1.0; semantic projection 1.0;
  authority false-positive rate 0/11).
- [ ] Taste real-world held-out interval evidence, implicit-evidence qualification,
  and exact released-package qualification remain open.
- [ ] Exact installed Adapt/package proof with Python and Node absent remains open;
  a copied source-built native binary smoke test is not that release receipt.
