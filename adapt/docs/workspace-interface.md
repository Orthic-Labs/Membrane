# Membrane Adapt — workspace interface

Adapt's installed boundary is native Rust. Production code must use typed crate and
protocol interfaces; it must not import the legacy Python workspace adapters or reach
through another subsystem's storage.

| Capability | Native boundary | Owner |
|---|---|---|
| Transcript events and provenance | `membrane-transcript` types and host adapters | Adapt transcript layer |
| Preference/Insight contracts and evaluation | `membrane-adapt` public Rust API | Adapt |
| Durable batch admission and scoped recall | native runtime/store APIs | Cortex |
| Scheduling and resident lifecycle | in-process runtime task hosted by Hub | Hub |
| Repository facts | typed Blueprint protocol calls | Blueprint |
| Document search | typed Ledger/runtime calls | Ledger |

Missing trust material, malformed protocol data, unavailable owner services, and stale
or unsealed records fail closed. The Python `workspace_runtime` module and scripts under
`adapt/src/adapt/` remain migration/differential inputs only; they are not an approved
installed interface. Their release exclusion is an N10 packaging obligation, not an
assumption made by this document.
