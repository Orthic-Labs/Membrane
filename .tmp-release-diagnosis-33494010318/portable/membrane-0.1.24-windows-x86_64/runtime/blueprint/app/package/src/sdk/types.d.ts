// D32: TypeScript declarations for the public SDK surface.

export interface BlueprintServiceInput {
  repoId?: string;
  generation?: string;
  allowStale?: boolean;
  timeoutMs?: number;
}

export interface SearchInput extends BlueprintServiceInput {
  query: string;
  limit?: number;
}

export interface AnchorInput extends BlueprintServiceInput {
  anchor: string;
  depth?: number;
  budget?: number;
  cursor?: string;
}

export interface PathInput extends BlueprintServiceInput {
  from: string;
  to: string;
  maxDepth?: number;
  budget?: number;
  cursor?: string;
}

export interface BlueprintResult {
  schemaVersion: number;
  generationId?: string;
  freshnessReceipt?: unknown;
}

export interface BlueprintSearchResult extends BlueprintResult {
  kind: "search";
  query: string;
  results: Array<{ id: string; kind: string; name: string | null; path: string }>;
}

export class BlueprintClient {
  constructor(options?: { endpoint?: string; contract?: string | null });
  status(input?: BlueprintServiceInput): Promise<BlueprintResult>;
  search(input: SearchInput): Promise<BlueprintSearchResult>;
  resolve(input: { nodeId: string } & BlueprintServiceInput): Promise<BlueprintResult>;
  recall(input: BlueprintServiceInput & { task?: string; query?: string; limit?: number }): Promise<BlueprintResult>;
  expand(input: AnchorInput): Promise<BlueprintResult>;
  impact(input: AnchorInput): Promise<BlueprintResult>;
  path(input: PathInput): Promise<BlueprintResult>;
  architecture(input: BlueprintServiceInput & { view?: "summary" | "flows" | "projection" | "changes"; budget?: number; maxFlows?: number; cursor?: string; snapshot?: string; sinceGeneration?: string; treeish?: string | { base: string; head?: string } }): Promise<BlueprintResult>;
  documentTruth(input: BlueprintServiceInput & { claimId?: string; limit?: number }): Promise<BlueprintResult>;
  federate(input: { repositories: Array<{ repoId: string; repoRoot?: string; generation?: string }>; allowedRepoIds?: string[]; operation: "search" | "recall" | "impact" | "architecture"; query?: Record<string, unknown> }): Promise<BlueprintResult>;
  close(): Promise<void>;
}

export class EmbeddedBlueprintClient {
  constructor(options?: { allowEmbeddedRoot?: boolean; outDir?: string });
  search(input: SearchInput): Promise<BlueprintSearchResult>;
  status(input?: BlueprintServiceInput): Promise<BlueprintResult>;
  path(input: PathInput): Promise<BlueprintResult>;
  architecture(input: BlueprintServiceInput & { view?: "summary" | "flows" | "projection" | "changes"; budget?: number; maxFlows?: number; cursor?: string }): Promise<BlueprintResult>;
  federate(input: { repositories: Array<{ repoId: string; repoRoot?: string; generation?: string }>; operation: "search" | "recall" | "impact" | "architecture"; query?: Record<string, unknown> }): Promise<BlueprintResult>;
  close(): Promise<void>;
}

export function defineProvider(provider: Record<string, unknown>): Readonly<Record<string, unknown>>;
export function definePlugin(plugin: Record<string, unknown>): Readonly<Record<string, unknown>>;
export const PLUGIN_TYPES: readonly string[];
export const PROTOCOL_VERSION: number;
