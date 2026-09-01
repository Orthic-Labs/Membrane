// Portable `.agent/manifest.json` path validation — repo-relative only.

const ABSOLUTE_INDICATORS = [
  { pattern: /[A-Za-z]:[\\/]/, code: "windows_drive" },
  { pattern: /\/Users\//, code: "mac_users" },
  { pattern: /\/home\//, code: "linux_home" },
  { pattern: /\/Volumes\//, code: "mac_volumes" },
  { pattern: /\\\\/, code: "unc_path" },
];

export function validatePortableRelativePath(path, fieldName) {
  const errors = [];
  const raw = String(path ?? "").trim();
  if (!raw) {
    errors.push(`${fieldName} is empty`);
    return errors;
  }
  const normalized = raw.replaceAll("\\", "/");
  if (normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized)) {
    errors.push(`${fieldName} is absolute`);
  }
  if (/(?:^|\/)\.\.(?:\/|$)/.test(normalized)) {
    errors.push(`${fieldName} contains parent traversal`);
  }
  if (normalized.includes("\0")) {
    errors.push(`${fieldName} contains a null byte`);
  }
  return errors;
}

export function findAbsolutePathIndicators(value) {
  const raw = JSON.stringify(value);
  const findings = [];
  for (const indicator of ABSOLUTE_INDICATORS) {
    if (indicator.pattern.test(raw)) findings.push(indicator.code);
  }
  return findings;
}

export function validatePortableManifest(manifest) {
  const errors = [];
  if (!manifest || typeof manifest !== "object") {
    return ["manifest is not a JSON object"];
  }
  if (manifest.schemaVersion !== 1) {
    errors.push(`unsupported schemaVersion ${manifest.schemaVersion}; expected 1`);
  }
  const pathFindings = findAbsolutePathIndicators(manifest);
  if (pathFindings.length > 0) {
    errors.push(`manifest contains absolute machine path indicators: ${pathFindings.join(", ")}`);
  }
  for (const [key, path] of Object.entries(manifest.artifacts ?? {})) {
    errors.push(...validatePortableRelativePath(path, `artifacts.${key}`));
  }
  for (const [index, path] of (manifest.humanDocs ?? []).entries()) {
    errors.push(...validatePortableRelativePath(path, `humanDocs[${index}]`));
  }
  if (manifest.entrypoint !== undefined) {
    errors.push(...validatePortableRelativePath(manifest.entrypoint, "entrypoint"));
  }
  if (!manifest?.repo?.sourceHash) {
    errors.push("manifest is missing repo.sourceHash");
  }
  return errors;
}
