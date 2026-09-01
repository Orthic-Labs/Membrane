export function normalizeRepoPath(value) {
  return String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "").normalize("NFC");
}

/** Total repository-path order: canonical bytes, then pre-NFC slash-normalized bytes. */
export function compareRepoPaths(left, right) {
  const canonical = Buffer.compare(Buffer.from(normalizeRepoPath(left), "utf8"), Buffer.from(normalizeRepoPath(right), "utf8"));
  if (canonical) return canonical;
  const raw = (value) => String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "");
  return Buffer.compare(Buffer.from(raw(left), "utf8"), Buffer.from(raw(right), "utf8"));
}
