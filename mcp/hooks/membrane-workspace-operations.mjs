// Membrane-owned workspace memory behavior. This layer invokes Cortex's public
// contracts; it contains no Arcane or research admission policy.
import { createHash } from "node:crypto";
import { appendFileSync, existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { createRequire } from "node:module";
import { get as httpGet } from "node:http";
import { request as httpRequest } from "node:http";
import { typedStatus } from "./membrane-hook-runtime.mjs";
import { createContinuityClient } from "../host/continuity.mjs";
import { diagnosticsRequest } from "../lib/diagnostics-client.mjs";
import { isVerificationCommand } from "../lib/verification-command.mjs";

const require = createRequire(import.meta.url);
const DEFAULT_CONTEXT_ADAPTER = require("../host/context-adapter.cjs");
const SERVICE_LIFECYCLE = "hub-child";

function workspaceRoot(event, env = process.env) {
  const requested = env.WORKSPACE_ROOT || event.payload.cwd || event.payload.working_directory || process.cwd();
  return resolve(String(requested));
}

function pendingPath(root, sessionId) {
  const digest = createHash("sha256").update(String(sessionId || "missing-session")).digest("hex").slice(0, 24);
  return join(root, "tools", ".cache", "memory", "checkpoint-pending", `${digest}.json`);
}

function inside(parent, candidate) {
  const rel = relative(parent, candidate);
  return rel && !rel.startsWith("..") && !resolve(parent, rel).startsWith(`${parent}/..`);
}

function claudeMemoryRoot(root, home = homedir()) {
  const slug = root.replaceAll(":", "-").replaceAll("\\", "-").replaceAll("/", "-");
  return resolve(home, ".claude", "projects", slug, "memory");
}

function toolName(event) {
  return String(event.payload.tool_name || event.payload.toolName || "");
}

function audit(hook, outcome, detail, home = homedir()) {
  try {
    const directory = join(home, ".claude", "usage-data", "hook-audit");
    mkdirSync(directory, { recursive: true });
    appendFileSync(join(directory, `${hook}.jsonl`), `${JSON.stringify({ at: new Date().toISOString(), hook, outcome, ...detail })}\n`, "utf8");
  } catch {}
}

function frontmatterField(text, key) {
  if (typeof text !== "string" || !text.startsWith("---")) return "";
  const end = text.indexOf("\n---", 3);
  if (end < 0) return "";
  const match = new RegExp(`^${key}:\\s*(.+?)$`, "m").exec(text.slice(3, end));
  return match?.[1]?.trim() || "";
}

function observablePort(root) {
  const configured = Number(process.env.MEMBRANE_PORT || 47851);
  if (Number.isInteger(configured) && configured >= 1024 && configured <= 65535) return configured;
  try {
    const runtime = JSON.parse(readFileSync(join(root, "tools", "lib", "memory", "runtime.json"), "utf8"));
    if (runtime.schemaVersion === 1 && runtime.serviceId === "membrane-local-v1" && Number.isInteger(runtime.port)) return runtime.port;
  } catch {}
  return 47851;
}

function activeTrace(root, sessionId) {
  const directory = process.env.MEMBRANE_ACTIVE_TRACE_DIR || join(root, "tools", ".cache", "memory", "active-traces");
  const key = createHash("sha256").update(String(sessionId)).digest("hex");
  try { return JSON.parse(readFileSync(join(directory, `${key}.json`), "utf8")).trace_id || null; } catch { return null; }
}

function postObservable(root, event, trace, { signal } = {}) {
  let token;
  try { token = readFileSync(process.env.MEMBRANE_API_TOKEN_FILE || join(root, "tools", ".cache", "memory", "api-token"), "utf8").trim(); } catch { return Promise.resolve(false); }
  const session = String(event.sessionId);
  const responseDigest = createHash("sha256").update(JSON.stringify(event.payload.tool_response ?? null)).digest("hex");
  const policyPath = join(root, "tools", ".cache", "memory", "active-policy.json");
  let policy = "membrane-tool-observer-v1";
  try { policy = readFileSync(policyPath); } catch {}
  const body = JSON.stringify({ events: [{
    schema: "membrane.observable-event.v1",
    installation_id: "tool-observer",
    client_id: process.env.MEMBRANE_CLIENT === "claude" ? "claude_code" : (process.env.MEMBRANE_CLIENT || "codex"),
    session_id: session,
    task_id: `task-${createHash("sha256").update(`${session}:${trace}`).digest("hex").slice(0, 24)}`,
    turn_id: `turn-${trace}`,
    trace_id: trace,
    event_id: `evt-${createHash("sha256").update(`${session}:${trace}:${Date.now()}`).digest("hex").slice(0, 32)}`,
    event_type: "tool_receipt",
    origin: "tool",
    content_ref_or_digest: `sha256:${responseDigest}`,
    timestamp: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    completeness: { observed: true, tool: true },
    policy_snapshot_digest: `sha256:${createHash("sha256").update(policy).digest("hex")}`,
  }] });
  return new Promise((done) => {
    const request = httpRequest({ hostname: "127.0.0.1", port: observablePort(root), path: "/v1/telemetry/observable-events:batch", method: "POST", signal, timeout: 1000,
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", "Content-Length": Buffer.byteLength(body) } }, (response) => {
      response.resume(); response.once("end", () => done(response.statusCode >= 200 && response.statusCode < 300));
    });
    request.once("error", () => done(false));
    request.once("timeout", () => request.destroy());
    request.end(body);
  });
}

const DURABLE_PATTERNS = [
  /\bfrom now on\b/i, /\bnever\s+(?:do|use|run|call|trust|assume|skip|revert|fabricate)\b/i,
  /\b(?:^|new\s+)?rule(?:\s+is)?:\s*\w/im, /\bthe right way (?:is|to)\b/i,
  /\bdon't (?:ever|ask|do that|repeat|paraphrase|guess)\b/i, /\bnext time,?\s+(?:do|don't|use|skip)\b/i,
];
const LOADED_CONTENT_MARKERS = ["<command-name>", "<command-message>", "<system-reminder>", "<command-output>", "Base directory for this skill:", "Contents of D:", "Contents of C:", "# claudeMd", "## Output format"];

function stopAdvisory(event, root, home = homedir()) {
  const transcriptPath = event.payload.transcript_path;
  if (typeof transcriptPath !== "string" || !transcriptPath) return null;
  let lines;
  try { lines = readFileSync(transcriptPath, "utf8").trim().split("\n").slice(-100); } catch { return null; }
  const entries = [];
  for (const line of lines) try { entries.push(JSON.parse(line)); } catch {}
  const memoryRoot = claudeMemoryRoot(root, home);
  let writes = 0;
  const messages = [];
  for (const entry of entries) {
    if (entry.type === "user") {
      const content = entry.message?.content;
      const text = Array.isArray(content) ? content.filter((block) => block?.type === "text").map((block) => block.text).join(" ") : content;
      if (typeof text === "string" && !/^\s*(?:yes|ok|okay|sure|cool|thanks|perfect|great|nice|done)[\s.!]*$/i.test(text)
        && !LOADED_CONTENT_MARKERS.some((marker) => text.slice(0, 600).includes(marker))) messages.push(text);
    }
    if (entry.type === "assistant") for (const block of entry.message?.content || []) {
      const file = block?.input?.file_path;
      if (block?.type === "tool_use" && ["Write", "Edit"].includes(block.name) && typeof file === "string" && inside(memoryRoot, resolve(file))) writes += 1;
    }
  }
  if (writes) return null;
  const hits = messages.slice(-15).filter((message) => DURABLE_PATTERNS.some((pattern) => pattern.test(message)));
  if (!hits.length) return null;
  return `[HOOK:memory-nag:advisory]\nWhy: ${hits.length} durable correction/confirmation signal(s) this session, no memory written\nRequired: save a memory file before ending if this should persist`;
}

export function durableWorkspaceFile(root, file, home = homedir()) {
  if (typeof file !== "string" || !file) return null;
  let absolute;
  try { absolute = realpathSync(resolve(file)); } catch { return null; }
  if (!absolute.endsWith(".md")) return null;
  const allowedRoots = [resolve(root, "memory"), resolve(root, ".agent", "okf"), claudeMemoryRoot(root, home)];
  if (allowedRoots.some((allowed) => inside(allowed, absolute))) return absolute;
  return inside(root, absolute) && absolute.endsWith("/start-here.md") ? absolute : null;
}

function memoryScope(root, file) {
  const rel = relative(root, file).replaceAll("\\", "/");
  return rel.startsWith("memory/") ? "global" : `repo:${createHash("sha256").update(root).digest("hex").slice(0, 16)}`;
}

function defaultProbeStatus(root) {
  let port = Number(process.env.MEMBRANE_PORT || 47851);
  try {
    const runtime = JSON.parse(readFileSync(join(root, "tools", "lib", "memory", "runtime.json"), "utf8"));
    if (runtime.schemaVersion === 1 && runtime.serviceId === "membrane-local-v1" && Number.isInteger(runtime.port)) port = runtime.port;
  } catch {}
  if (!Number.isInteger(port) || port < 1024 || port > 65535) return Promise.resolve(false);
  return new Promise((resolveProbe) => {
    const request = httpGet({ hostname: "127.0.0.1", port, path: "/health", timeout: 800 }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => {
        try {
          const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
          resolveProbe(response.statusCode === 200 && body.ok === true);
        } catch { resolveProbe(false); }
      });
    });
    request.once("error", () => resolveProbe(false));
    request.once("timeout", () => { request.destroy(); resolveProbe(false); });
  });
}

function checkpointSnapshot(event, root) {
  const raw = event.payload;
  const transcriptRef = raw.transcript_ref || raw.transcriptRef || (raw.transcript_id ? { id: String(raw.transcript_id), host: raw.client || "host" } : null);
  return {
    schema_version: 1,
    checkpoint_id: `checkpoint/${createHash("sha256").update(String(event.sessionId || "missing-session")).digest("hex").slice(0, 24)}/${Date.now()}`,
    client: "codex",
    session_id: event.sessionId || "missing-session",
    scope_id: root,
    created_at_ms: Date.now(),
    trigger: String(raw.trigger || "unknown"),
    transcriptRef,
  };
}

function redactSummary(summary) {
  return String(summary || "").split(/\r?\n/).map((line) => (
    /(api[_-]?key|password|secret|token\s*=)/i.test(line) ? "[redacted sensitive summary line]" : line
  )).join("\n");
}

function ingestArgs(root, file, event) {
  const rel = relative(root, file).replaceAll("\\", "/");
  const blueprint = rel.startsWith(".agent/okf/") || file.endsWith("/start-here.md");
  const args = [
    "put", basename(file, ".md"), "--scope", memoryScope(root, file), "--file", file,
    "--artifact-family", blueprint ? "blueprint" : "memory",
    "--producer", "membrane_hook",
    "--record-type", blueprint ? "blueprint_concept" : "markdown_memory",
    "--authority", "A0", "--influence-class", "data_only",
  ];
  if (event.sessionId) args.push("--session", event.sessionId);
  const trace = event.payload.trace_id || event.payload.tool_use_id;
  if (trace) args.push("--trace", String(trace));
  return args;
}

function diagnosticsIdentity(event, root, env = process.env) {
  return {
    repoId: env.MEMBRANE_DIAGNOSTICS_REPO_ID || basename(root),
    worktreeId: env.MEMBRANE_DIAGNOSTICS_WORKTREE_ID || createHash("sha256").update(resolve(root)).digest("hex").slice(0, 16),
  };
}

/** Fence enforcement is active for opted-in workspaces only: an explicit
 * environment switch or a workspace marker file. Hosts that never adopted
 * Membrane keep running tests/builds untouched; hosts that did are enforced
 * fail-closed at every verification boundary (design §10/§11). */
function fenceEnforcementEnabled(root, env = process.env) {
  if (env.MEMBRANE_DIAGNOSTICS_ENFORCE === "1") return true;
  if (env.MEMBRANE_DIAGNOSTICS_ENFORCE === "0") return false;
  return existsSync(join(root, ".agent", "diagnostics-enforce.json"));
}

export function createWorkspaceMemoryOperations(options = {}) {
  const { contextAdapter = DEFAULT_CONTEXT_ADAPTER, rootFor = workspaceRoot, continuityService, probeStatus = defaultProbeStatus, home = homedir(), postObservation = postObservable, diagnosticsPost = diagnosticsRequest } = options;
  // Cortex subprocess execution is never an installed-hook default. Tests may
  // inject a seam; production must provide the current continuity service.
  const runCortex = typeof options.runCortex === "function" ? options.runCortex : null;
  const continuity = createContinuityClient({ service: continuityService });

  // Shared Semantic Edit Fence gate body (design §10): one implementation for
  // both the PreToolUse test/build boundary and the Stop completion boundary.
  // Fail-closed when enforcement is enabled: missing evidence is never permission.
  const evaluateFenceGate = async (event, boundary) => {
    const root = rootFor(event);
    const env = process.env;
    if (!fenceEnforcementEnabled(root, env)) return typedStatus("skipped", "fence_enforcement_not_enabled");
    const { repoId, worktreeId } = diagnosticsIdentity(event, root, env);
    const tool = toolName(event);
    const command = String(event.payload.tool_input?.command || event.payload.command || "");
    if (boundary === "test_build_boundary" && !isVerificationCommand(command, tool)) {
      return typedStatus("skipped", "fence_not_applicable");
    }
    try {
      const status = await diagnosticsPost(`/diagnostics/workspace/status?repoId=${encodeURIComponent(repoId)}&worktreeId=${encodeURIComponent(worktreeId)}`, { method: "GET", timeoutMs: 800 });
      if (!status || !status.ok) {
        audit("diagnostics-fence", "blocked", { repoId, worktreeId, tool, boundary, reason: "workspace_not_open" });
        return typedStatus("blocked", "fence_not_cleared", { repoId, worktreeId, boundary, detail: `semantic edit fence not cleared at ${boundary}: workspace not open or diagnostics unavailable` });
      }
      const body = status.body ?? {};
      // Fail-closed: any uncleared fence, missing epoch, or not-cleared state blocks.
      // The old `workspace_not_open => skipped` and `no sealed epoch => skipped`
      // paths are wrong for an opted-in workspace: missing evidence is not permission.
      if (body.fenceCleared !== true) {
        audit("diagnostics-fence", "blocked", { repoId, worktreeId, tool, boundary });
        return typedStatus("blocked", "fence_not_cleared", { repoId, worktreeId, boundary, detail: `semantic edit fence not cleared at ${boundary}: run diagnostics snapshot.await and repair before tests/builds/completion` });
      }
      return typedStatus("available", "fence_cleared", { repoId, worktreeId, boundary });
    } catch {
      audit("diagnostics-fence", "blocked", { repoId, worktreeId, tool, boundary, reason: "diagnostics_unreachable" });
      return typedStatus("blocked", "fence_not_cleared", { repoId, worktreeId, boundary, detail: "diagnostics unreachable: fail-closed, repair and re-query before tests/builds/completion" });
    }
  };

  return Object.freeze({
    async status(event) {
      const healthy = await probeStatus(rootFor(event));
      return typedStatus(healthy ? "available" : "unavailable", healthy ? "cortex_healthy" : "cortex_unavailable", { lifecycle: SERVICE_LIFECYCLE });
    },
    async rearm(event) {
      if (event.payload.source !== "compact" || !event.sessionId) return typedStatus("skipped", "rearm_not_applicable");
      const root = rootFor(event);
      const safeSession = String(event.sessionId).replace(/[^A-Za-z0-9_-]/g, "_");
      const database = process.env.CORTEX_DB || join(root, "tools", ".cache", "memory", "cortex-engine.db");
      try { unlinkSync(join(dirname(database), "recall-seen", `${safeSession}.json`)); } catch {}
      return typedStatus("available", "recall_rearmed");
    },
    async recall(event) {
      const root = rootFor(event);
      const request = contextAdapter.buildRequest(event.payload, root);
      const result = await contextAdapter.runClient(request, root);
      const additionalContext = contextAdapter.render(result);
      return typedStatus(result.state === "context_enforced" ? "available" : "unavailable", result.state === "context_enforced" ? "memory_recalled" : "memory_unavailable", { additionalContext });
    },
    async preCompact(event) {
      const root = rootFor(event);
      const path = pendingPath(root, event.sessionId);
      mkdirSync(resolve(path, ".."), { recursive: true });
      writeFileSync(path, `${JSON.stringify(checkpointSnapshot(event, root))}\n`, "utf8");
      return typedStatus("available", "checkpoint_prepared");
    },
    async postCompact(event, context) {
      const root = rootFor(event);
      const path = pendingPath(root, event.sessionId);
      if (!existsSync(path)) return typedStatus("skipped", "checkpoint_not_pending");
      let checkpoint;
      try { checkpoint = JSON.parse(readFileSync(path, "utf8")); } catch { return typedStatus("unavailable", "checkpoint_invalid"); }
      checkpoint.summary = redactSummary(event.payload.compact_summary);
      checkpoint.expires_at_ms = Date.now() + 86_400_000;
      if (typeof continuityService === "function") {
        const saved = await continuity.checkpoint({
          id: checkpoint.checkpoint_id,
          sessionId: checkpoint.session_id,
          taskId: checkpoint.task_id,
          transcriptRef: checkpoint.transcriptRef,
          authority: { level: "A0", scope: checkpoint.scope_id },
          trigger: checkpoint.trigger,
          repository: root,
        });
        if (saved.state === "available") unlinkSync(path);
        return typedStatus(saved.state === "available" ? "available" : "unavailable", saved.state === "available" ? "checkpoint_captured" : saved.reason, { continuity: true });
      }
      if (!runCortex) return typedStatus("unavailable", "continuity_service_unavailable", { continuity: false });
      const saved = await runCortex(root, ["checkpoint", "save"], JSON.stringify(checkpoint), context);
      if (saved) unlinkSync(path);
      return typedStatus(saved ? "available" : "unavailable", saved ? "checkpoint_captured" : "checkpoint_save_failed");
    },
    async ingest(event, context) {
      const root = rootFor(event);
      const file = durableWorkspaceFile(root, event.payload.tool_input?.file_path || event.payload.file_path, home);
      if (!file) return typedStatus("skipped", "ingest_not_applicable");
      if (!runCortex) return typedStatus("unavailable", "memory_service_unavailable");
      const saved = await runCortex(root, ingestArgs(root, file, event), "", context);
      return typedStatus(saved ? "available" : "unavailable", saved ? "memory_ingested" : "memory_ingest_failed");
    },
    async bump(event) {
      if (toolName(event) !== "Read") return typedStatus("skipped", "bump_not_applicable");
      const root = rootFor(event);
      const raw = event.payload.tool_input?.file_path;
      const target = durableWorkspaceFile(root, raw, home);
      const memoryRoot = claudeMemoryRoot(root, home);
      if (!target || !inside(memoryRoot, target) || ["MEMORY.md", "MEMORY-archive.md", "MEMORY-cold.md"].includes(basename(target))) return typedStatus("skipped", "bump_not_applicable");
      let source;
      try { source = readFileSync(target, "utf8"); } catch { return typedStatus("skipped", "bump_unreadable"); }
      if (!source.startsWith("---")) return typedStatus("skipped", "bump_no_frontmatter");
      const end = source.indexOf("\n---", 3);
      if (end < 0) return typedStatus("skipped", "bump_no_frontmatter");
      const today = new Date().toISOString().slice(0, 10);
      const front = source.slice(3, end);
      const match = /^last_accessed:\s*(\S+)/m.exec(front);
      if (match?.[1] === today) return typedStatus("skipped", "bump_current");
      const updated = match ? `${front.slice(0, match.index)}last_accessed: ${today}${front.slice(match.index + match[0].length)}` : `${front.trimEnd()}\nlast_accessed: ${today}\n`;
      const temporary = `${target}.bumptmp`;
      try { writeFileSync(temporary, `---${updated}${source.slice(end)}`, "utf8"); renameSync(temporary, target); }
      catch { try { unlinkSync(temporary); } catch {} return typedStatus("unavailable", "bump_write_failed"); }
      audit("memory-bump", "pass", { path: target }, home);
      return typedStatus("available", "memory_access_bumped");
    },
    async conflict(event) {
      if (toolName(event) !== "Write") return typedStatus("skipped", "conflict_not_applicable");
      const root = rootFor(event);
      const target = durableWorkspaceFile(root, event.payload.tool_input?.file_path, home);
      const content = event.payload.tool_input?.content;
      const memoryRoot = claudeMemoryRoot(root, home);
      if (!target || !inside(memoryRoot, target) || typeof content !== "string") return typedStatus("skipped", "conflict_not_applicable");
      const domain = frontmatterField(content, "domain");
      if (!domain) return typedStatus("skipped", "conflict_no_domain");
      let conflicts = [];
      try { conflicts = readdirSync(memoryRoot).filter((name) => name.endsWith(".md") && resolve(memoryRoot, name) !== target).filter((name) => frontmatterField(readFileSync(join(memoryRoot, name), "utf8"), "domain") === domain); } catch {}
      if (!conflicts.length) return typedStatus("skipped", "conflict_none");
      const additionalContext = `[HOOK:memory-conflict:advisory]\nWhy: new memory in domain '${domain}' has ${conflicts.length} same-domain sibling(s)\nRequired: verify this is not a duplicate; consider editing existing memory\n\n${conflicts.slice(0, 8).map((name) => `  - ${name}`).join("\n")}`;
      audit("memory-conflict", "advisory", { path: target, domain, sibling_count: conflicts.length }, home);
      return typedStatus("available", "memory_conflict", { additionalContext });
    },
    async observe(event, context) {
      if (toolName(event) !== "Bash" || !event.sessionId) return typedStatus("skipped", "observe_not_applicable");
      const root = rootFor(event);
      const trace = activeTrace(root, event.sessionId);
      if (!trace) return typedStatus("skipped", "observe_no_trace");
      const posted = await postObservation(root, event, trace, context);
      return typedStatus(posted ? "available" : "unavailable", posted ? "tool_observed" : "tool_observe_failed");
    },
    async observeMutation(event) {
      const root = rootFor(event);
      const env = process.env;
      const { repoId, worktreeId } = diagnosticsIdentity(event, root, env);

      const tool = toolName(event);
      const input = event.payload.tool_input ?? {};
      const rawCandidates = [];
      const pushCandidate = (value) => {
        if (typeof value === "string" && value.trim()) rawCandidates.push(value.trim());
      };
      pushCandidate(input.file_path);
      pushCandidate(input.filePath);
      pushCandidate(event.payload.file_path);
      pushCandidate(event.payload.filePath);
      if (Array.isArray(input.edits)) {
        for (const edit of input.edits) {
          if (edit && typeof edit === "object") {
            pushCandidate(edit.file_path);
            pushCandidate(edit.filePath);
          }
        }
      }
      const patchText = String(input.patch ?? input.content ?? "");
      if (tool === "apply_patch" || patchText.includes("*** Begin Patch") || patchText.includes("diff --git") || patchText.includes("*** Update File") || patchText.includes("*** Add File")) {
        for (const line of patchText.split(/\r?\n/)) {
          let match = line.match(/^\*\*\*\s*(?:Update|Add)\s+File:\s*(.+)$/);
          if (match) pushCandidate(match[1].trim());
          match = line.match(/^---\s+a\/(.+)$/);
          if (match) pushCandidate(match[1].trim());
          match = line.match(/^\+\+\+\s+b\/(.+)$/);
          if (match) pushCandidate(match[1].trim());
          match = line.match(/^diff\s+--git\s+a\/(.+)\s+b\/(.+)$/);
          if (match) { pushCandidate(match[1].trim()); pushCandidate(match[2].trim()); }
        }
      }
      const seen = new Set();
      const changedPaths = [];
      for (const candidate of rawCandidates) {
        let rel;
        try {
          const absolute = resolve(root, candidate);
          rel = relative(root, absolute).replaceAll("\\", "/");
          if (rel.startsWith("..") || rel.startsWith("/")) rel = candidate.replaceAll("\\", "/").replace(/^\.\//, "");
        } catch {
          rel = candidate.replaceAll("\\", "/").replace(/^\.\//, "");
        }
        if (!rel) rel = candidate.replaceAll("\\", "/").replace(/^\.\//, "");
        if (!rel || seen.has(rel)) continue;
        seen.add(rel);
        changedPaths.push(rel);
      }
      if (changedPaths.length === 0) return typedStatus("skipped", "diagnostics_not_applicable");
      const changedFileHashes = [];
      for (const relPath of changedPaths) {
        const absolute = resolve(root, relPath);
        let bytes;
        try { bytes = readFileSync(absolute); } catch { continue; }
        const hash = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
        changedFileHashes.push({ path: relPath, hash });
      }
      if (changedFileHashes.length === 0) return typedStatus("skipped", "diagnostics_target_unreadable");
      const manifestHash = createHash("sha256");
      const sortedHashes = [...changedFileHashes].sort((left, right) => left.path.localeCompare(right.path));
      for (const entry of sortedHashes) {
        manifestHash.update(entry.path);
        manifestHash.update("\0");
        manifestHash.update(entry.hash);
        manifestHash.update("\0");
      }
      const sourceManifestDigest = `sha256:${manifestHash.digest("hex")}`;
      const digestOf = (bytes) => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
      const digestOfFiles = (paths) => {
        const hasher = createHash("sha256");
        let found = false;
        for (const candidatePath of paths) {
          try {
            const bytes = readFileSync(join(root, candidatePath));
            hasher.update(candidatePath);
            hasher.update("\0");
            hasher.update(bytes);
            hasher.update("\0");
            found = true;
          } catch {}
        }
        if (!found) hasher.update("empty");
        return `sha256:${hasher.digest("hex")}`;
      };
      const projectConfigDigest = digestOfFiles(["package.json", "pyproject.toml", "Cargo.toml", "pnpm-workspace.yaml", ".agent/config.json", "blueprint.json", "membrane.json"]);
      const toolchainDigest = digestOf(Buffer.from(`${process.version}\0${process.platform}\0${process.arch}`, "utf8"));
      const sandboxPolicyDigest = digestOfFiles(["tools/lib/memory/runtime.json", ".agent/sandbox.json", "sandbox.json"]);
      let nextEpoch = 0;
      let parentEpoch = undefined;
      try {
        const statusResult = await diagnosticsPost(`/diagnostics/workspace/status?repoId=${encodeURIComponent(repoId)}&worktreeId=${encodeURIComponent(worktreeId)}`, { method: "GET", timeoutMs: 800 });
        if (statusResult.ok && statusResult.body && typeof statusResult.body === "object") {
          const latest = statusResult.body.latestSealedEpoch;
          if (Number.isInteger(latest) && latest >= 0) {
            nextEpoch = latest + 1;
            parentEpoch = latest;
          } else if (latest === null || latest === undefined) {
            const configured = Number.parseInt(env.MEMBRANE_DIAGNOSTICS_EPOCH ?? "", 10);
            if (Number.isInteger(configured) && configured >= 0) {
              nextEpoch = configured;
              parentEpoch = configured > 0 ? configured - 1 : undefined;
            } else {
              nextEpoch = 0;
            }
          }
        } else {
          throw new Error("status_unavailable");
        }
      } catch {
        const configured = Number.parseInt(env.MEMBRANE_DIAGNOSTICS_EPOCH ?? "", 10);
        const counterPath = join(root, "tools", ".cache", "diagnostics", "observed-epoch.json");
        let current = null;
        try {
          const raw = readFileSync(counterPath, "utf8");
          const parsed = JSON.parse(raw);
          if (Number.isInteger(parsed.epoch) && parsed.epoch >= 0) current = parsed.epoch;
        } catch {}
        if (Number.isInteger(current)) {
          nextEpoch = current + 1;
          parentEpoch = current;
        } else if (Number.isInteger(configured) && configured >= 0) {
          nextEpoch = configured + 1;
          parentEpoch = configured;
        } else {
          nextEpoch = 0;
        }
        try {
          mkdirSync(dirname(counterPath), { recursive: true });
          writeFileSync(counterPath, `${JSON.stringify({ repoId, worktreeId, epoch: nextEpoch, parentEpoch: parentEpoch ?? null, updatedAt: new Date().toISOString() })}\n`, "utf8");
        } catch {}
      }
      try {
        const counterPath = join(root, "tools", ".cache", "diagnostics", "observed-epoch.json");
        mkdirSync(dirname(counterPath), { recursive: true });
        writeFileSync(counterPath, `${JSON.stringify({ repoId, worktreeId, epoch: nextEpoch, parentEpoch: parentEpoch ?? null, updatedAt: new Date().toISOString() })}\n`, "utf8");
      } catch {}
      const epoch = {
        schemaVersion: "workspace-epoch.v1",
        repoId,
        worktreeId,
        epoch: nextEpoch,
        ...(parentEpoch !== undefined ? { parentEpoch } : {}),
        sourceManifestDigest,
        changedPaths,
        changedFileHashes,
        projectConfigDigest,
        toolchainDigest,
        sandboxPolicyDigest,
        origin: "observed_hook",
      };
      let posted;
      try {
        // Bind the session to the exact canonical root at first mutation
        // (design §3 WorkspaceEngineKey). Idempotent: reopening is a no-op.
        await diagnosticsPost("/diagnostics/workspace/open", {
          method: "POST",
          body: { repoId, worktreeId, projectRoot: resolve(root) },
          timeoutMs: 1200,
        });
        posted = await diagnosticsPost("/diagnostics/mutation/registerObserved", { method: "POST", body: { repoId, worktreeId, epoch }, timeoutMs: 1200 });
      } catch {
        audit("diagnostics-registerObserved", "degraded", { code: "diagnostics_register_failed", paths: changedPaths.join(",") });
        return typedStatus("unavailable", "diagnostics_register_failed", { paths: changedPaths, hashes: changedFileHashes });
      }
      if (!posted.ok) {
        audit("diagnostics-registerObserved", "degraded", { code: posted.error.code, paths: changedPaths.join(",") });
        return typedStatus("unavailable", posted.error.code, { paths: changedPaths, hashes: changedFileHashes });
      }
      const primaryPath = changedPaths[0];
      const primaryHash = changedFileHashes.find((entry) => entry.path === primaryPath)?.hash ?? changedFileHashes[0]?.hash;
      return typedStatus("available", "mutation_observed", { path: primaryPath, hash: primaryHash, paths: changedPaths, hashes: changedFileHashes });
    },
    async nag(event) {
      const root = rootFor(event);
      const additionalContext = stopAdvisory(event, root, home);
      if (!additionalContext) return typedStatus("skipped", "nag_not_applicable");
      audit("memory-nag", "advisory", { session_id: event.sessionId || null }, home);
      return typedStatus("available", "memory_nag", { additionalContext });
    },
    async postToolUseFailure(event) {
      const reason = String(event.payload?.error?.message || event.payload?.error || event.payload?.reason || "");
      if (!reason.trim()) return typedStatus("skipped", "failure_not_applicable");
      // Bounded, secret-safe: only a redacted, content-free summary is retained.
      const summary = redactSummary(reason);
      audit("memory-failure", "observed", { session_id: event.sessionId || null }, home);
      return typedStatus("available", "failure_observed", { summaryLength: summary.length, contentFree: true });
    },
    async taskCompleted(event) {
      if (!event.sessionId) return typedStatus("skipped", "episode_not_applicable");
      const outcomes = Array.isArray(event.payload?.outcomes) ? event.payload.outcomes : [];
      const bounded = outcomes.slice(0, 64);
      const outcomeDigest = createHash("sha256").update(JSON.stringify(bounded)).digest("hex");
      audit("memory-episode", "observed", { session_id: event.sessionId, outcome_count: outcomes.length }, home);
      return typedStatus("available", "episode_captured", { outcomeDigest: `sha256:${outcomeDigest}`, contentFree: true });
    },
    async sessionEnd(event) {
      audit("memory-session-end", "observed", { session_id: event.sessionId || null }, home);
      return typedStatus("available", "session_closed");
    },
    // Host fence enforcement for CodeRight / Claude Code / Codex (design §10:
    // "Hosts enforce; providers report evidence"). Runs at the PreToolUse
    // test/build/release boundary in opted-in workspaces. Fail-closed: when
    // enforcement is on but diagnostics cannot answer, the boundary blocks.
    // Returns state "blocked" so the entrypoint can translate it into a real
    // host deny decision — registration alone is not enforcement.
    async enforceFence(event) {
      return evaluateFenceGate(event, "test_build_boundary");
    },

    // Completion boundary (design §10): a Stop may not end the session with
    // sealed-but-uncleared bytes. Same gating and fail-closed semantics.
    async enforceCompletion(event) {
      return evaluateFenceGate(event, "completion");
    },
  });
}
