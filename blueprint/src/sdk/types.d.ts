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
  architecture(input: BlueprintServiceInput & { budget?: number }): Promise<BlueprintResult>;
  documentTruth(input: BlueprintServiceInput & { claimId?: string; limit?: number }): Promise<BlueprintResult>;
  close(): Promise<void>;
}

export class EmbeddedBlueprintClient {
  constructor(options?: { allowEmbeddedRoot?: boolean; outDir?: string });
  search(input: SearchInput): Promise<BlueprintSearchResult>;
  status(input?: BlueprintServiceInput): Promise<BlueprintResult>;
  close(): Promise<void>;
}

export function defineProvider(provider: Record<string, unknown>): Readonly<Record<string, unknown>>;
export function definePlugin(plugin: Record<string, unknown>): Readonly<Record<string, unknown>>;
export const PLUGIN_TYPES: readonly string[];
export const PROTOCOL_VERSION: number;
