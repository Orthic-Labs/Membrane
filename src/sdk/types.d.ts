// D32: TypeScript declarations for the public SDK surface.

export interface CortexServiceInput {
  repoId?: string;
  generation?: string;
  allowStale?: boolean;
  timeoutMs?: number;
}

export interface SearchInput extends CortexServiceInput {
  query: string;
  limit?: number;
}

export interface AnchorInput extends CortexServiceInput {
  anchor: string;
  depth?: number;
  budget?: number;
  cursor?: string;
}

export interface CortexResult {
  schemaVersion: number;
  generationId?: string;
  freshnessReceipt?: unknown;
}

export interface CortexSearchResult extends CortexResult {
  kind: "search";
  query: string;
  results: Array<{ id: string; kind: string; name: string | null; path: string }>;
}

export class CortexClient {
  constructor(options?: { endpoint?: string; contract?: string | null });
  status(input?: CortexServiceInput): Promise<CortexResult>;
  search(input: SearchInput): Promise<CortexSearchResult>;
  resolve(input: { nodeId: string } & CortexServiceInput): Promise<CortexResult>;
  orient(input: CortexServiceInput & { task?: string; query?: string; limit?: number }): Promise<CortexResult>;
  expand(input: AnchorInput): Promise<CortexResult>;
  impact(input: AnchorInput): Promise<CortexResult>;
  architecture(input: CortexServiceInput & { budget?: number }): Promise<CortexResult>;
  documentTruth(input: CortexServiceInput & { claimId?: string; limit?: number }): Promise<CortexResult>;
  close(): Promise<void>;
}

export class EmbeddedCortexClient {
  constructor(options?: { allowEmbeddedRoot?: boolean; outDir?: string });
  search(input: SearchInput): Promise<CortexSearchResult>;
  status(input?: CortexServiceInput): Promise<CortexResult>;
  close(): Promise<void>;
}

export function defineProvider(provider: Record<string, unknown>): Readonly<Record<string, unknown>>;
export function definePlugin(plugin: Record<string, unknown>): Readonly<Record<string, unknown>>;
export const PLUGIN_TYPES: readonly string[];
export const PROTOCOL_VERSION: number;
