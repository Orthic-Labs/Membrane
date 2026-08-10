export interface ManifestV1 {
  schemaVersion: 1;
  productId: "cortex" | "membrane";
  displayName: string;
  productVersion: string;
  hubCompatRange: string;
  installRoot: string;
  serviceStart: string[];
  serviceStop: string[];
  statusEndpoint: { host: string; port: number; authHeader: string; authToken: string };
  icon: string;
}
