# MEM-001 federation parity corpus

`manifest.v1.json` is the corpus authority. Its `inputResolution` binds baseline
paths to exact Git revision `cfe43fbd8306f28c3851a039b57e38997fb29461` & names
any current-workspace override explicitly. Each input carries SHA-256 of bytes
at that declared location; missing current paths do not silently retarget
historical evidence. Case IDs are unique, lexical, & immutable; later packets
consume cases by ID.

Cases are simulations or immutable snapshots. They do not invoke providers,
write Cortex storage, start processes, or duplicate effects. `expected` records
normative semantics: candidate identity/content, trust, provenance, protected,
exact, recoverable, resolver, warnings, omissions, generations, error codes,
and canonical ordering.

Comparison uses only named rules in `manifest.v1.json`: UUIDs/timestamps,
temporary roots, elapsed measurements, and scheduler completion order are
normalized at their explicit paths. Repository roots normalize to a canonical
scope ID; provider completion is sorted by `providerOrder`. No blanket ignore
is permitted. Allowed migration delta is limited to process metadata and
content-free diagnostic detail, as listed in the manifest.

Source evidence is baseline code/tests/fixtures only. Resolve `evidenceRefs`
against `inputResolution`, not current checkout, unless path is an explicit
workspace override. Future packet owners are recorded per case; integrators
seal legacy observations separately.
