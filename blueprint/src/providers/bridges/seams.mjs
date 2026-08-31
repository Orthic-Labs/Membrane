import { defineProvider } from "../index.mjs";

export const BRIDGE_PROVIDER_ID = "blueprint-bridge-seams";
export const BRIDGE_PROVIDER_VERSION = "explicit-seams-v1";

const RULES = Object.freeze([
  { kind: "FFI", extensions: ["rs"], pattern: /\bextern\s+"C"/, target: () => "C ABI" },
  { kind: "FFI", extensions: ["py"], pattern: /\bctypes\.(?:CDLL|PyDLL|WinDLL)\(\s*["']([^"']+)["']/, target: (match) => match[1] },
  { kind: "JNI", extensions: ["java", "kt"], pattern: /\bnative\s+[A-Za-z_$][\w$<>\[\]]*\s+([A-Za-z_$][\w$]*)\s*\(/, target: (match) => match[1] },
  { kind: "JNI", extensions: ["c", "cc", "cpp", "cxx", "h", "hpp"], pattern: /\bJNIEXPORT\b[\s\S]*?\bJNICALL\s+([A-Za-z_$][\w$]*)/, target: (match) => match[1] },
  { kind: "cgo", extensions: ["go"], pattern: /^\s*import\s+"C"\s*$/, target: () => "C" },
  { kind: "gRPC", extensions: ["proto"], pattern: /^\s*service\s+([A-Za-z_]\w*)\s*\{?/, target: (match) => match[1] },
  { kind: "gRPC", extensions: ["proto"], pattern: /^\s*rpc\s+([A-Za-z_]\w*)\s*\(/, target: (match) => match[1] },
  { kind: "PInvoke", extensions: ["cs"], pattern: /\[DllImport\(\s*["']([^"']+)["']/, target: (match) => match[1] },
  { kind: "WASM", extensions: ["rs"], pattern: /#\[wasm_bindgen(?:\([^\]]*\))?\]/, target: () => "wasm-bindgen" },
  { kind: "WASM", extensions: ["js", "jsx", "ts", "tsx", "mjs", "cjs"], pattern: /\bWebAssembly\.(?:instantiate|instantiateStreaming|compile)\s*\(/, target: () => "WebAssembly" },
  { kind: "COM", extensions: ["cs"], pattern: /\[(?:ComImport|Guid)\b/, target: () => "COM" },
  { kind: "COM", extensions: ["cs", "cpp", "cc", "cxx", "c", "ps1", "vbs"], pattern: /\b(?:CoCreateInstance|CreateObject)\s*\(\s*["']?([^"')\s]+)?/, target: (match) => match[1] ?? "COM" },
]);

function extension(path) {
  const dot = String(path ?? "").lastIndexOf(".");
  return dot < 0 ? "" : path.slice(dot + 1).toLowerCase();
}

function sourceLine(raw, ext) {
  const trimmed = raw.trim();
  if (!trimmed
    || trimmed.startsWith("//")
    || (trimmed.startsWith("#") && !(ext === "rs" && trimmed.startsWith("#[")))
    || trimmed.startsWith("/*")
    || trimmed.startsWith("*")) return null;
  return raw.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/, "").trimEnd();
}

export function scanExplicitBridgeSeams(file) {
  const ext = extension(file?.path);
  const seams = [];
  const lines = Array.isArray(file?.lines) ? file.lines : String(file?.text ?? "").split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = sourceLine(lines[index], ext);
    if (line === null) continue;
    for (const rule of RULES) {
      if (!rule.extensions.includes(ext)) continue;
      const match = line.match(rule.pattern);
      if (!match) continue;
      seams.push({
        bridgeKind: rule.kind,
        target: rule.target(match),
        path: file.path,
        line: index + 1,
      });
    }
  }
  return seams;
}

function safeId(value) {
  return String(value).replace(/[^A-Za-z0-9_.-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 80) || "seam";
}

export const bridgeSeamProvider = defineProvider({
  id: BRIDGE_PROVIDER_ID,
  version: BRIDGE_PROVIDER_VERSION,
  kind: "repository-evidence",
  protocolRange: ">=1 <2",
  capabilities: ["ffi", "jni", "cgo", "grpc", "pinvoke", "wasm", "com"],
  permissions: { filesystem: "repo-read", network: "none", process: "none" },
  probe(context = {}) {
    return { state: Array.isArray(context.files) ? "available" : "unavailable", reason: Array.isArray(context.files) ? null : "source_files_absent" };
  },
  collect(context = {}) {
    const nodes = [];
    const edges = [];
    for (const file of context.files ?? []) {
      for (const seam of scanExplicitBridgeSeams(file)) {
        const nodeId = `bridge:${seam.path}:${seam.line}:${seam.bridgeKind}:${safeId(seam.target)}`;
        const evidence = [{
          path: seam.path,
          startLine: seam.line,
          endLine: seam.line,
          contentHash: file.contentHash ?? null,
          bridgeKind: seam.bridgeKind,
          target: seam.target,
        }];
        nodes.push({
          id: nodeId,
          kind: "bridge",
          labels: ["CrossLanguageBridge", seam.bridgeKind],
          name: `${seam.bridgeKind}:${seam.target}`,
          qualifiedName: `${seam.path}:${seam.line}:${seam.bridgeKind}:${seam.target}`,
          path: seam.path,
          bridgeKind: seam.bridgeKind,
          targetName: seam.target,
          confidence: 1,
          provider: BRIDGE_PROVIDER_ID,
          factProvider: { id: BRIDGE_PROVIDER_ID, version: BRIDGE_PROVIDER_VERSION },
          evidence,
        });
        edges.push({
          id: `edge:CONTAINS:file:${seam.path}->${nodeId}`,
          kind: "CONTAINS",
          source: `file:${seam.path}`,
          target: nodeId,
          confidence: 1,
          confidenceTier: "EXACT_RESOLUTION",
          resolved: true,
          provider: BRIDGE_PROVIDER_ID,
          factProvider: { id: BRIDGE_PROVIDER_ID, version: BRIDGE_PROVIDER_VERSION },
          evidence,
        });
      }
    }
    return {
      nodes,
      edges,
      reports: [],
      summary: { provider: BRIDGE_PROVIDER_ID, filesConsidered: (context.files ?? []).length, seams: nodes.length },
    };
  },
});
