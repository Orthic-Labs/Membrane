import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, lstatSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { validateCycloneDxSbom, validateInTotoSlsaProvenance } from "@rightkit/release/supply-chain-evidence.mjs";

if (process.platform !== "win32") throw new Error("Windows candidate check must run on Windows");
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const artifactRoot = process.env.RIGHT_GIT_ARTIFACT_ROOT;
if (!artifactRoot) throw new Error("RIGHT_GIT_ARTIFACT_ROOT is required");
const candidateManifestPath = join(artifactRoot, "candidate.json");
if (!existsSync(candidateManifestPath)) throw new Error("candidate.json is missing");
const candidate = JSON.parse(readFileSync(candidateManifestPath, "utf8"));
const signingStatus = candidate.signing?.status;
if (candidate.schemaVersion !== 1 || !["membrane-signed-release-candidate", "membrane-unsigned-release-candidate"].includes(candidate.kind) || candidate.product !== "membrane" || candidate.target !== "windows-x86_64") throw new Error("candidate identity is invalid");
if (!/^[0-9a-f]{40}$/.test(candidate.sourceCommit)) throw new Error("candidate source commit is invalid");
if (!/^\d+$/.test(candidate.github?.runId ?? "") || !/^\d+$/.test(candidate.github?.runAttempt ?? "")) throw new Error("candidate GitHub run identity is invalid");
if (!new RegExp(`^membrane-[0-9A-Za-z.+-]+-windows-x86_64-${signingStatus ?? "(?:signed|unsigned)"}\\.zip$`).test(candidate.archive?.name)) throw new Error("candidate archive name is invalid");
if (!candidate.startedAt || Number.isNaN(Date.parse(candidate.startedAt))) throw new Error("candidate start time is invalid");
for (const name of ["membrane-hub.exe", "cortex.exe", "membrane.exe", "membrane-client.exe", "membrane-tray.exe", "membrane-daemon.exe"]) {
  if (!candidate.files?.[name]) throw new Error(`candidate executable closure missing: ${name}`);
}
for (const name of Object.keys(candidate.files ?? {})) {
  if (/^mcp[\\/]/i.test(name) || /(?:^|[\\/])blueprint[\\/]/i.test(name) || /\.(?:mjs|cjs|js)$/i.test(name)) {
    throw new Error(`candidate contains retired JavaScript runtime path: ${name}`);
  }
}
for (const name of [
  "plugin.json", "mcp.json", ".mcp.json", "mcp_config.json",
  "hooks/hooks.json", "hooks/codex-hooks.json",
  ".claude-plugin/plugin.json", ".claude-plugin/marketplace.json",
  ".codex-plugin/plugin.json", ".antigravity-plugin/plugin.json",
  ".antigravity-plugin/mcp_config.json", ".agents/plugins/marketplace.json",
]) {
  if (!candidate.files?.[name]) throw new Error(`candidate client projection closure missing: ${name}`);
}
for (const name of ["LICENSE", "THIRD_PARTY_NOTICES.md"]) if (!candidate.files?.[name]) throw new Error(`candidate legal closure missing: ${name}`);
if (!Object.keys(candidate.files ?? {}).some((name) => name.startsWith("skills/membrane/"))) throw new Error("candidate skill closure is missing");
if (!Object.keys(candidate.files ?? {}).some((name) => name.startsWith("runtime/"))) throw new Error("candidate runtime closure is missing");
const archive = join(artifactRoot, candidate.archive.name);
if (!existsSync(archive)) throw new Error("candidate archive is missing");
const bytes = readFileSync(archive);
if (bytes.length !== candidate.archive.size) throw new Error("candidate archive size mismatch");
if (createHash("sha256").update(bytes).digest("hex") !== candidate.archive.sha256) throw new Error("candidate archive digest mismatch");
if (signingStatus !== "signed" && signingStatus !== "unsigned") throw new Error("candidate signing status is invalid");
const manifestPath = join(artifactRoot, "release-manifest.json");
const sbomPath = join(artifactRoot, "sbom.json");
if (!existsSync(manifestPath) || !existsSync(sbomPath)) throw new Error("candidate qualification evidence is incomplete");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const sbom = JSON.parse(readFileSync(sbomPath, "utf8"));
if (manifest.schema !== "membrane.release-evidence.v1" || manifest.product !== "Membrane Hub" || manifest.artifact?.sha256?.toLowerCase() !== candidate.archive.sha256.toLowerCase() || manifest.artifact?.path !== candidate.archive.name) throw new Error("release manifest is not candidate-archive-bound");
if (sbom.schema !== "membrane.sbom.v1" || sbom.artifact?.sha256?.toLowerCase() !== candidate.archive.sha256.toLowerCase() || sbom.artifact?.path !== candidate.archive.name) throw new Error("SBOM is not candidate-archive-bound");
if (manifest.signing?.status !== signingStatus || sbom.signing?.status !== signingStatus) throw new Error("candidate signing label is inconsistent");
const evidenceNames = new Set([`sbom-windows-x86_64-${signingStatus}.cdx.json`, `provenance-windows-x86_64-${signingStatus}.intoto.jsonl`]);
if (!Array.isArray(candidate.evidence) || candidate.evidence.length !== evidenceNames.size || candidate.evidence.some((item) => !evidenceNames.delete(item.name)) || evidenceNames.size) throw new Error("candidate evidence closure is invalid");
for (const evidence of candidate.evidence) {
  const path = join(artifactRoot, evidence.name);
  if (!existsSync(path)) throw new Error(`candidate evidence missing: ${evidence.name}`);
  const evidenceBytes = readFileSync(path);
  if (evidenceBytes.length !== evidence.size || createHash("sha256").update(evidenceBytes).digest("hex") !== evidence.sha256) throw new Error(`candidate evidence digest mismatch: ${evidence.name}`);
}
const expectedSubject = { name: candidate.archive.name, sha256: candidate.archive.sha256 };
validateCycloneDxSbom(join(artifactRoot, `sbom-windows-x86_64-${signingStatus}.cdx.json`), { expectedFile: expectedSubject });
const provenance = validateInTotoSlsaProvenance(join(artifactRoot, `provenance-windows-x86_64-${signingStatus}.intoto.jsonl`), { expectedSubject });
if (!provenance.predicate.buildDefinition.resolvedDependencies[0].uri.endsWith(`@${candidate.sourceCommit}`)) throw new Error("candidate provenance source mismatch");
const extracted = mkdtempSync(join(tmpdir(), "membrane-candidate-check-"));
try {
  // Resolve the System32 bsdtar explicitly: PATH often finds GNU tar (Git
  // Bash) first, which reads the colon in an absolute Windows path (C:\...)
  // as a remote host spec and fails with "Cannot connect to C:".
  const tarExe = join(process.env.SystemRoot ?? "C:\Windows", "System32", "tar.exe");
  const unpack = spawnSync(tarExe, ["-xf", archive, "-C", extracted], { windowsHide: true });
  if (unpack.error || unpack.status !== 0) throw new Error("candidate archive extraction failed");
  const walk = (root) => readdirSync(root).flatMap((entry) => {
    const path = join(root, entry);
    if (lstatSync(path).isSymbolicLink()) throw new Error(`candidate contains link: ${path}`);
    return statSync(path).isDirectory() ? walk(path) : [path];
  });
  const actual = Object.fromEntries(walk(extracted).map((path) => [relative(extracted, path).replaceAll("\\", "/"), createHash("sha256").update(readFileSync(path)).digest("hex")]));
  if (JSON.stringify(actual) !== JSON.stringify(candidate.files)) throw new Error("candidate file closure mismatch");
  if (existsSync(join(extracted, "mcp")) || existsSync(join(extracted, "runtime", "blueprint"))) throw new Error("candidate archive includes retired runtime tree");
  // Authenticated MCP transport probe against the exact staged binaries.
  // `hook --help` only proved argv parsing; the resident contract is an
  // authenticated Streamable HTTP exchange — initialize, tools/list, and a
  // harmless status call — plus the same exchange forwarded through the
  // staged membrane-client stdio transport. The staged engine is launched
  // into an isolated state root on a private loopback port so this checks
  // the shipped binaries without touching any installed state.
  // MEMBRANE_CANDIDATE_TRANSPORT_PROBE=0 exists only for fixture-level
  // handoff tests that cannot compile real binaries; real candidate gates
  // never set it, and the skip is printed in the result payload.
  if (process.env.MEMBRANE_CANDIDATE_TRANSPORT_PROBE === "0") {
    console.error("candidate MCP transport probe skipped (MEMBRANE_CANDIDATE_TRANSPORT_PROBE=0)");
  } else {
    await probeCandidateMcp(extracted);
  }
} finally {
  rmSync(extracted, { recursive: true, force: true });
}
const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8", windowsHide: true });
if (head.error || head.status !== 0 || head.stdout.trim() !== candidate.sourceCommit) throw new Error("candidate source commit does not match checkout");
console.log(JSON.stringify({
  ok: true,
  sourceCommit: candidate.sourceCommit,
  archive: candidate.archive,
  transportProbe: process.env.MEMBRANE_CANDIDATE_TRANSPORT_PROBE === "0" ? "skipped" : "passed",
}));

// ---------------------------------------------------------------------------
// Authenticated MCP protocol probe (initialize -> tools/list -> status call)
// against the exact staged binaries. The engine is launched into an isolated
// MEMBRANE_STATE_ROOT on a private loopback port; the loopback credential is
// read from the state root the engine itself provisions.
// ---------------------------------------------------------------------------

function freePort() {
  return new Promise((resolvePromise, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolvePromise(port));
    });
  });
}

function parseMcpBody(text) {
  // Streamable HTTP answers either JSON or an SSE `data:` frame.
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith("{")) return JSON.parse(trimmed);
  const data = trimmed
    .split("\n")
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice(5).trim())
    .join("");
  return data ? JSON.parse(data) : null;
}

async function probeCandidateMcp(root) {
  const port = await freePort();
  const stateRoot = join(root, ".candidate-state");
  const engine = spawn(join(root, "membrane.exe"), [], {
    env: {
      ...process.env,
      MEMBRANE_RUNTIME_ORIGIN: "installed",
      MEMBRANE_STATE_ROOT: stateRoot,
      MEMBRANE_PORT: String(port),
      MEMBRANE_HTTP_PORT: String(port),
    },
    stdio: ["ignore", "ignore", "ignore"],
    windowsHide: true,
  });
  let spawnError = null;
  engine.once("error", (error) => { spawnError = error; });
  try {
    const tokenPath = join(stateRoot, "tools", ".cache", "memory", "api-token");
    const deadline = Date.now() + 90_000;
    let token = null;
    while (Date.now() < deadline) {
      if (spawnError) {
        throw new Error(`candidate engine failed to launch: ${spawnError.message}`);
      }
      if (engine.exitCode !== null) {
        throw new Error(`candidate engine exited ${engine.exitCode} before the MCP probe`);
      }
      if (existsSync(tokenPath)) {
        const value = readFileSync(tokenPath, "utf8").trim();
        if (/^[0-9a-f]{64}$/.test(value)) {
          token = value;
          break;
        }
      }
      await delay(500);
    }
    if (!token) throw new Error("candidate engine did not provision its loopback credential");

    // Direct authenticated exchange (the transport Codex/Claude bind to).
    const endpoint = `http://127.0.0.1:${port}/mcp`;
    let sessionId = null;
    const rpc = async (message) => {
      const headers = {
        "Authorization": `Bearer ${token}`,
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
      };
      if (sessionId) headers["Mcp-Session-Id"] = sessionId;
      const response = await fetch(endpoint, {
        method: "POST",
        headers,
        body: JSON.stringify(message),
        signal: AbortSignal.timeout(20_000),
      });
      const next = response.headers.get("mcp-session-id");
      if (next) sessionId = next;
      if (!response.ok && response.status !== 202) {
        throw new Error(`mcp ${message.method ?? message.id} HTTP ${response.status}`);
      }
      return parseMcpBody(await response.text());
    };
    const init = await rpc({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2025-06-18",
        capabilities: {},
        clientInfo: { name: "membrane-candidate-check", version: candidate.version ?? "0" },
      },
    });
    if (!init?.result?.serverInfo && !init?.result?.protocolVersion) {
      throw new Error("candidate MCP initialize returned no result");
    }
    await rpc({ jsonrpc: "2.0", method: "notifications/initialized" });
    const listed = await rpc({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} });
    const tools = (listed?.result?.tools ?? []).map((tool) => tool.name);
    for (const expected of ["pull", "push"]) {
      if (!tools.includes(expected)) {
        throw new Error(`candidate tools/list is missing ${expected}`);
      }
    }
    // Harmless status call: `pull` without arguments must produce a typed
    // response (result or JSON-RPC error) — either proves authenticated
    // dispatch; a transport failure does not.
    const status = await rpc({ jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "pull", arguments: {} } });
    if (!status || status.id !== 3 || (!status.result && !status.error)) {
      throw new Error("candidate status call returned no JSON-RPC response");
    }

    // The staged transport client must forward the same exchange to the
    // resident engine using only MEMBRANE_PORT + MEMBRANE_BEARER_TOKEN.
    const client = spawn(join(root, "membrane-client.exe"), ["stdio-mcp"], {
      env: { ...process.env, MEMBRANE_PORT: String(port), MEMBRANE_BEARER_TOKEN: token },
      stdio: ["pipe", "pipe", "ignore"],
      windowsHide: true,
    });
    try {
      const stdioResponse = await new Promise((resolvePromise, reject) => {
        let buffer = "";
        const timer = setTimeout(() => reject(new Error("membrane-client stdio-mcp probe timed out")), 20_000);
        client.stdout.on("data", (chunk) => {
          buffer += chunk.toString("utf8");
          const line = buffer.split("\n").find((entry) => entry.trim().startsWith("{"));
          if (line) {
            clearTimeout(timer);
            resolvePromise(line.trim());
          }
        });
        client.once("exit", (code) => {
          clearTimeout(timer);
          reject(new Error(`membrane-client stdio-mcp exited ${code} before responding`));
        });
        client.once("error", (error) => {
          clearTimeout(timer);
          reject(error);
        });
        client.stdin.write(`${JSON.stringify({
          jsonrpc: "2.0",
          id: 10,
          method: "initialize",
          params: {
            protocolVersion: "2025-06-18",
            capabilities: {},
            clientInfo: { name: "membrane-candidate-check", version: candidate.version ?? "0" },
          },
        })}\n`);
      });
      const parsed = JSON.parse(stdioResponse);
      if (!parsed?.result?.serverInfo && !parsed?.result?.protocolVersion) {
        throw new Error("membrane-client stdio-mcp initialize returned no result");
      }
    } finally {
      client.kill();
    }
  } finally {
    if (engine.exitCode === null) {
      spawnSync("taskkill.exe", ["/PID", String(engine.pid), "/T", "/F"], { windowsHide: true });
    }
    rmSync(stateRoot, { recursive: true, force: true });
  }
}
