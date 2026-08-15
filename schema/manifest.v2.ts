export interface ManifestV2 {
  schemaVersion: 2;
  productId: "cortex" | "membrane";
  displayName: string;
  productVersion: string;
  hubCompatRange: string;
  installRoot: string;
  serviceStart: string[];
  serviceStop: string[];
  icon: string;
  artifactDigest: `sha256:${string}`;
}
