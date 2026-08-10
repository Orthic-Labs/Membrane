export type SectionState = "available" | "degraded" | "unavailable";
export interface SectionV1 {
  state: SectionState;
  reason: string;
  items?: Array<Record<string, unknown> | string | number | boolean>;
  evidence?: string;
  resolver?: string;
  observedAtUnixMs?: number;
}
export interface SnapshotV1 {
  schemaVersion: 1;
  productId: "cortex" | "membrane";
  observedAtUnixMs: number;
  sections: Record<string, SectionV1>;
  stale?: boolean;
  cacheAgeMs?: number;
}
