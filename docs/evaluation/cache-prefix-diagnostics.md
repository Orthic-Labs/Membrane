# Cache-prefix diagnostics

`/federate` model output carries `cachePrefixDiagnostic` beside `packet`.
`prefixDigest` hashes canonical, model-delivered prefix fields; `traceId` plus
omissions are excluded. `stableBlockOrder`, block digests, plus metadata
digests let a dashboard compare two receipts without retaining packet text.

When supplied a prior packet, `cacheBreak` deterministically names first
changed `blockId` or `metadataField`; `volatilitySource` reports that cause.
With only one packet, `volatilitySource` is `none`.
