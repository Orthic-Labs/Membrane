function signatureText(node) {
  const kind = (node.labels ?? [node.kind ?? "symbol"])[0] ?? node.kind ?? "symbol";
  const name = node.qualifiedName ?? node.name ?? node.id;
  const declared = node.signature ?? node.rawDeclaredType ?? node.semanticSignature ?? null;
  const receiver = node.receiverType ?? node.declaringType ?? null;
  const line = node.evidence?.[0]?.startLine ?? null;
  return [
    String(kind),
    String(name),
    receiver ? `receiver=${receiver}` : null,
    declared ? `:: ${String(declared).replace(/\s+/g, " ").trim()}` : null,
    node.path ? `@ ${node.path}${line ? `:${line}` : ""}` : null,
  ].filter(Boolean).join(" ");
}

export function projectSymbolSignatures(generation, { limit = 200, pathPrefix = null, kinds = null } = {}) {
  const cap = Math.max(1, Math.min(1000, Number(limit) || 200));
  const allowedKinds = Array.isArray(kinds) && kinds.length ? new Set(kinds.map((value) => String(value).toLowerCase())) : null;
  const rows = (generation?.nodes ?? [])
    .filter((node) => node.kind === "symbol" || node.kind === "class" || (node.labels ?? []).some((label) => ["Function", "Method", "Class", "Interface", "Trait", "Struct", "Test", "Screen"].includes(label)))
    .filter((node) => !pathPrefix || String(node.path ?? "").startsWith(pathPrefix))
    .filter((node) => !allowedKinds || [node.kind, ...(node.labels ?? [])].some((kind) => allowedKinds.has(String(kind).toLowerCase())))
    .map((node) => ({
      id: node.id,
      portableId: node.portableId ?? null,
      kind: node.kind,
      labels: node.labels ?? [],
      name: node.name ?? null,
      qualifiedName: node.qualifiedName ?? null,
      path: node.path ?? null,
      line: node.evidence?.[0]?.startLine ?? null,
      signature: signatureText(node),
      evidence: node.evidence ?? [],
    }))
    .sort((a, b) => String(a.path).localeCompare(String(b.path)) || String(a.qualifiedName ?? a.name ?? a.id).localeCompare(String(b.qualifiedName ?? b.name ?? b.id)));
  return {
    schemaVersion: 1,
    kind: "symbol-signatures",
    generationId: generation?.manifest?.generationId ?? null,
    signatures: rows.slice(0, cap),
    truncated: rows.length > cap,
    omissions: rows.length > cap ? [{ reason: "signature_limit", limit: cap }] : [],
  };
}
