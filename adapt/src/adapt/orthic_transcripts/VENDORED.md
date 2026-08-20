# Vendored: `orthic_transcripts` (TranscriptEventV1 parser)

This directory is a **verbatim copy** of the `orthic_transcripts` package that
originally lived in the parent workspace's Legion tree.

- Source: `legion/src/lib/orthic_transcripts` at commit
  `8db792e4ff69155e1c9dc79ef5f27cd474490c0a` (2026-08-19).
- **Owned by Adapt.** Adapt is its own system and does not depend on Legion.
  A tool that sits inside Legion and that Adapt calls is copied into Adapt;
  imports never point back at `tools/skills/legion/...` or `legion/src/lib/...`.
- **Do not pull updates from Legion.** This copy is Adapt's responsibility
  alone going forward. If the parser must change, change it here.
- **Keep `PARSER_DIGEST` stable.** The digest is computed at import time over
  every `.py` file under this root (`compute_parser_digest`, content + basename,
  order-stable). It is embedded in every candidate's `sourceParserDigest` and
  in `extraction_contract()["parser_sha256"]`, so receipts stay valid as long as
  this tree is unchanged. Do not add stray `.py` files here or the digest drifts.
- The `tests/` subtree and fixtures are part of the faithful copy and are
  included only to keep the file set (and thus `PARSER_DIGEST`) identical to
  the source tree. `driver.py` / `selftest.py` are Legion's CLI/test scaffolding
  and are inert to Adapt runtime; nothing in Adapt imports them.
