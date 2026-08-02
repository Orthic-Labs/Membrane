# Changelog

## 2026-08-02 — Vector dispatch v2 default-on

- Flipped `CRYPT_VECTOR_DISPATCH_V2` to default-on: resident in-process f32 index (`retrieve_hybrid_indexed`) is now the live recall path on both hosts.
- Retained immediate environment-variable fallback: `CRYPT_VECTOR_DISPATCH_V2=0|false|off|legacy` restores legacy scalar-A `retrieve_hybrid` routing on next store open.
- Windows host confirmation + flag-on/flag-off acceptance green before flip; A remains fail-closed fallback for mixed dimensions, absent projection, or query mismatch.

## 2026-08-01 — Membrane P1 honesty

- Documented reserved admission lanes as explicit cross-provider score policy (no pretend shared scale).
- Public surfaces rename to Membrane (headers, telemetry registry provider, state/telemetry titles); RightContext kept as compatibility alias.
- CLI/serve write paths refuse hand-typed single-token scopes that would fork the corpus.
- README/STATE declare internal-mirror / workspace-coupled posture (not a standalone public product).

## 2026-07-30 — MCP 2026-07-28 dual-era stdio

- Migrated public MCP stdio to exact `@modelcontextprotocol/server@2.0.0` with one factory serving legacy `2025-03-26` clients & modern `2026-07-28` discovery.
- Added enforced input/output schemas, structured tool results with text fallback, & retained protocol-resource parity.
- Added bounded W3C `traceparent`, `tracestate`, & `baggage` propagation through `/federate`, including trace-ID request correlation.
- Tightened caller authorization to exact repository, root, scope descriptor, & scope ID binding; declared Node.js `>=20`.

## 2026-07-30 — RMS + Markdown Doc Spine absorption

- Absorbed RMS lifecycle v20, checkpoints, telemetry, migration/backout, doctor, native MCP/federation, installer, & typed virtual scopes into Crypt/Membrane.
- Added `DocOutlineV1`, `DocReadV1`, machine-local Markdown registration, hash-bound section reads, namespaced frontmatter, shadow replay, & fail-closed H2 fallback.
- Certified runtime root/Membrane `74b0ad52` / `944ea3ad`, engine tree `ac41729c4f8857756529a0832e0675e39dd52e9c740e28961fdb5ae358631a7f`, on macOS & Windows.
- Preserved Windows Q1–Q5 & installed-service proof in OWN-only evidence commit `6cd71abb89da454c179e990f6fb429ba21ab32b5`; attestation SHA-256 `9c4126d6dd5e2963b0846575da1bdd21cfc9788c740a4b95495790ec98e80af1`.
- Qualification-only Windows test corrections remain isolated in suite authority `de214878`; runtime behavior & P0–P4 evidence were not replayed.
- CodeRight now consumes `crypt` & `crypt-core` from exact Membrane revision `944ea3ad`; retired root-repository pins are removed.
