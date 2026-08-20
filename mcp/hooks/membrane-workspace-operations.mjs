// Membrane-owned workspace memory behavior. This layer invokes Cortex's public
// contracts; it contains no Arcane or research admission policy.
import { createHash } from "node:crypto";
import { appendFileSync, existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { get as httpGet } from "node:http";
import { request as httpRequest } from "node:http";
import { typedStatus } from "./membrane-hook-runtime.mjs";

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
  const configured = Number(process.env.CORTEX_PORT || 47851);
  if (Number.isInteger(configured) && configured >= 1024 && configured <= 65535) return configured;
  try {
    const runtime = JSON.parse(readFileSync(join(root, "tools", "lib", "memory", "runtime.json"), "utf8"));
    if (runtime.schemaVersion === 1 && runtime.serviceId === "cortex-local-v1" && Number.isInteger(runtime.port)) return runtime.port;
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
  try { token = readFileSync(process.env.CORTEX_API_TOKEN_FILE || join(root, "tools", ".cache", "memory", "api-token"), "utf8").trim(); } catch { return Promise.resolve(false); }
  const session = String(event.sessionId);
  const responseDigest = createHash("sha256").update(JSON.stringify(event.payload.tool_response ?? null)).digest("hex");
  const policyPath = join(root, "tools", ".cache", "memory", "active-policy.json");
  let policy = "membrane-tool-observer-v1";
  try { policy = readFileSync(policyPath); } catch {}
  const body = JSON.stringify({ events: [{
    schema: "orthic.observable-event.v1",
    installation_id: "tool-observer",
    client_id: process.env.CORTEX_CLIENT === "claude" ? "claude_code" : (process.env.CORTEX_CLIENT || "codex"),
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

function defaultRunCortex(root, args, input, { signal } = {}) {
  const binary = process.env.CORTEX_BIN || join(root, "tools", "bin", "cortex");
  return new Promise((resolveRun) => {
    let child;
    let settled = false;
    const finish = (value) => { if (!settled) { settled = true; resolveRun(value); } };
    try {
      child = spawn(binary, args, { cwd: root, stdio: ["pipe", "ignore", "ignore"], windowsHide: true, signal });
    } catch { finish(false); return; }
    child.once("error", () => finish(false));
    child.once("close", (code) => finish(code === 0));
    child.stdin.once("error", () => finish(false));
    child.stdin.end(input || "");
  });
}

function defaultProbeStatus(root) {
  let port = Number(process.env.CORTEX_PORT || 47851);
  try {
    const runtime = JSON.parse(readFileSync(join(root, "tools", "lib", "memory", "runtime.json"), "utf8"));
    if (runtime.schemaVersion === 1 && runtime.serviceId === "cortex-local-v1" && Number.isInteger(runtime.port)) port = runtime.port;
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
  return {
    schema_version: 1,
    checkpoint_id: `checkpoint/${createHash("sha256").update(String(event.sessionId || "missing-session")).digest("hex").slice(0, 24)}/${Date.now()}`,
    client: "codex",
    session_id: event.sessionId || "missing-session",
    scope_id: root,
    created_at_ms: Date.now(),
    trigger: String(raw.trigger || "unknown"),
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

export function createWorkspaceMemoryOperations({ contextAdapter = DEFAULT_CONTEXT_ADAPTER, rootFor = workspaceRoot, runCortex = defaultRunCortex, probeStatus = defaultProbeStatus, home = homedir(), postObservation = postObservable } = {}) {
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
      const saved = await runCortex(root, ["checkpoint", "save"], JSON.stringify(checkpoint), context);
      if (saved) unlinkSync(path);
      return typedStatus(saved ? "available" : "unavailable", saved ? "checkpoint_captured" : "checkpoint_save_failed");
    },
    async ingest(event, context) {
      const root = rootFor(event);
      const file = durableWorkspaceFile(root, event.payload.tool_input?.file_path || event.payload.file_path, home);
      if (!file) return typedStatus("skipped", "ingest_not_applicable");
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
  });
}
