/** Closed, bounded, content-free section item (orthic.snapshot.v2). Mirrors
 * schema/snapshot.v2.schema.json `sections.*.items[]`. Arbitrary maps are
 * forbidden by the seam contract: an item carries only named scalar/evidence
 * handles, never source text, memory text, secrets, or nested content. */
export type ItemSeverity = "info" | "warning" | "error" | "critical";

export interface SnapshotItemV2 {
  label: string;
  kind?: string;
  count?: number;
  severity?: ItemSeverity;
  evidence?: string;
  resolver?: string;
  observedAtUnixMs?: number;
  stale?: boolean;
}

export type SectionState = "available" | "degraded" | "unavailable";

export interface SectionV2 {
  state: SectionState;
  reason: string;
  items?: SnapshotItemV2[];
  evidence?: string;
  resolver?: string;
  observedAtUnixMs?: number;
}

export interface SnapshotV2 {
  schemaVersion: 2;
  productId: "cortex" | "membrane";
  observedAtUnixMs: number;
  sections: Record<string, SectionV2>;
  stale?: boolean;
  cacheAgeMs?: number;
}