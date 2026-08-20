// MBR-012 — Canonical delivery serialization (public ESM surface).
//
// Delivered content is accounted on FOUR surfaces that must agree byte-for-byte:
// bytes, chars, tokens, and the content hash. Before this task the renderer
// measured chars as UTF-16 code units (String.length), the ledger measured
// bytes as UTF-8, tokens came from chars/4, and the hash was computed ad hoc —
// so the four surfaces disagreed on any non-ASCII content.
//
// The single implementation lives in context-renderer-lib.cjs
// (`canonicalDeliveryMetrics`) so the CommonJS Forge hook and this ESM surface
// can never drift. This module re-exports it plus the documented conventions.

import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const lib = require("./context-renderer-lib.cjs");

/** Schema id for the canonical delivery-metrics contract. */
export const DELIVERY_METRICS_SCHEMA = "membrane.delivery-metrics.v1";

/** The canonical conventions, pinned in schemas/delivery-metrics.v1.schema.json. */
export const DELIVERY_METRICS_CONVENTIONS = Object.freeze({
  encoding: "utf8",
  bytes: "utf8_byte_count",
  chars: "unicode_code_points",
  tokens: "ceil(bytes/4)",
  sha256: "hex_digest_of_utf8_bytes",
});

export const canonicalDeliveryMetrics = lib.canonicalDeliveryMetrics;
