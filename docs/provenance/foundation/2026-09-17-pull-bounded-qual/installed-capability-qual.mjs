// Installed-surface qualification: SessionStart hook + push/pull + byte-exact recall
// against the resident installed engine at the stable current root.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current";
const MEMBRANE = join(ROOT, "membrane-client.exe");
const CLIENT = join(ROOT, "membrane-client.exe");
const results = [];
const record = (name, ok, detail) => { results.push({ name, ok, detail }); console.log(`${ok ? "PASS" : "FAIL"} ${name}: ${detail}`); };

function mcp(requests, timeout = 60_000) {
  const r = spawnSync(MEMBRANE, ["stdio-mcp"], {
    input: requests.map((x) => JSON.stringify(x)).join("\n") + "\n",
    encoding: "utf8", timeout, windowsHide: true, maxBuffer: 64 * 1024 * 1024,
  });
  if (r.error || r.status !== 0) throw new Error(`stdio-mcp failed: ${r.error?.message || r.stderr || r.status}`);
  return String(r.stdout).trim().split(/\r?\n/).filter(Boolean).map((l) => JSON.parse(l));
}

// --- 1. SessionStart hook (installed payload -> resident engine /hook route) ---
{
  const payload = {
    hook_event_name: "SessionStart",
    session_id: "qual-sessionstart-" + Date.now(),
    source: "startup",
    cwd: "D:\\Claude\\membrane",
  };
  const r = spawnSync(CLIENT, ["hook"], { input: JSON.stringify(payload), encoding: "utf8", timeout: 25_000, windowsHide: true });
  const out = String(r.stdout || "").trim();
  let parsed = null;
  try { parsed = JSON.parse(out); } catch { /* not json */ }
  record("sessionstart_hook_transport", r.status === 0 && parsed !== null,
    `exit=${r.status} stdout=${out.slice(0, 300)}`);
  const inserted = parsed && (parsed.hookSpecificOutput || parsed.additionalContext || parsed.context || parsed.systemMessage || parsed.stdout);
  record("sessionstart_orientation_insertion", !!inserted, inserted ? JSON.stringify(inserted).slice(0, 400) : `no context payload: ${out.slice(0, 300)}`);
}

// --- 2. MCP initialize + tool registry (authenticated installed transport) ---
// scopeId-only caller writes to scope `membrane`; the registry-bound filesystem
// scopeDescriptor writes to `D--Claude-membrane`, which is the chain pull's
// cortex lane actually searches.
const caller = { root: "D:\\Claude\\membrane", repositoryId: "membrane", scopeId: "membrane" };
const fsCaller = { ...caller, scopeDescriptor: { kind: "filesystem", path: "membrane" } };
const init = [
  { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "installed-cap-qual", version: "1" } } },
  { jsonrpc: "2.0", method: "notifications/initialized", params: {} },
  { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} },
];
const res1 = mcp(init);
const listing = res1.find((x) => x.id === 2);
const toolNames = (listing?.result?.tools || []).map((t) => t.name).sort();
record("mcp_initialize_and_registry", toolNames.join(",") === "pull,push", `tools=${toolNames.join(",")}`);

// --- 3. push byte-exact recall ---
const marker = `qual-byte-exact-${Date.now()}`;
const body = `Byte-exactness probe ${marker}\nLine two with unicode: héllo wörld — 汉字 🚀\n\tTabs and  spaces\ttrailing   \n{"json":"nested","arr":[1,2,3]}\nFinal line`;
const sha = createHash("sha256").update(body, "utf8").digest("hex");
const pushReq = [
  { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "installed-cap-qual", version: "1" } } },
  { jsonrpc: "2.0", method: "notifications/initialized", params: {} },
  { jsonrpc: "2.0", id: 10, method: "tools/call", params: { name: "push", arguments: { repository: "membrane", caller, requestId: marker, body } } },
  // Filesystem-scoped twin: pull's cortex lane searches the filesystem scope
  // chain (D--Claude-membrane), not the caller-declared scopeId.
  { jsonrpc: "2.0", id: 11, method: "tools/call", params: { name: "push", arguments: { repository: "membrane", caller: fsCaller, requestId: marker + "-fs", body } } },
];
const res2 = mcp(pushReq);
const pushRes = res2.find((x) => x.id === 10);
const pushKind = pushRes?.result?.structuredContent?.result?.kind;
const pushData = pushRes?.result?.structuredContent?.result?.data || pushRes?.result?.structuredContent?.result;
const memoryId = pushData?.memoryId || pushData?.id || pushData?.memory_id;
const fsPushRes = res2.find((x) => x.id === 11);
const fsPushKind = fsPushRes?.result?.structuredContent?.result?.kind;
record("push_admission", pushKind === "success", `kind=${pushKind} memoryId=${memoryId} bodySha256=${sha} raw=${JSON.stringify(pushRes?.result?.structuredContent).slice(0, 400)}`);
record("push_admission_fs_scope", fsPushKind === "success", `kind=${fsPushKind} memoryId=${fsPushRes?.result?.structuredContent?.result?.data?.memoryId}`);

// --- 4. recall + byte-exact read ---
if (memoryId) {
  const recallReq = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "installed-cap-qual", version: "1" } } },
    { jsonrpc: "2.0", method: "notifications/initialized", params: {} },
    { jsonrpc: "2.0", id: 20, method: "tools/call", params: { name: "membrane_memory_read", arguments: { repository: "membrane", caller, id: memoryId, expectedContentHash: "sha256:" + sha } } },
    { jsonrpc: "2.0", id: 21, method: "tools/call", params: { name: "membrane_memory", arguments: { repository: "membrane", caller, operation: "recall", query: marker, bounds: { maxItems: 5 }, recipe: { name: "cortex.hybrid", version: 1 } } } },
  ];
  const res3 = mcp(recallReq);
  const readRes = res3.find((x) => x.id === 20);
  const recallRes = res3.find((x) => x.id === 21);
  const readData = readRes?.result?.structuredContent?.result?.data || readRes?.result?.structuredContent?.result;
  const readBody = readData?.body ?? readData?.memory?.body ?? readData?.content;
  const readSha = typeof readBody === "string" ? createHash("sha256").update(readBody, "utf8").digest("hex") : null;
  record("byte_exact_recall", readSha === sha, `readSha=${readSha} expected=${sha} readKind=${readRes?.result?.structuredContent?.result?.kind}`);
  record("memory_recall_finds_push", !!JSON.stringify(recallRes).includes(marker), `recall=${JSON.stringify(recallRes?.result?.structuredContent).slice(0, 400)}`);
}

// --- 5. pull ---
{
  const pullReq = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "installed-cap-qual", version: "1" } } },
    { jsonrpc: "2.0", method: "notifications/initialized", params: {} },
    { jsonrpc: "2.0", id: 30, method: "tools/call", params: { name: "pull", arguments: { task: `byte-exactness probe ${marker}`, taskId: "qual-task-1", sessionId: "qual-session-1", repository: "membrane", caller: fsCaller, budget: 8000, deadlineMs: 90000 } } },
  ];
  const res4 = mcp(pullReq, 120_000);
  const pullRes = res4.find((x) => x.id === 30);
  const sc = pullRes?.result?.structuredContent;
  const foundMarker = JSON.stringify(sc || {}).includes(marker);
  const pullResult = sc?.result || {};
  const pullData = pullResult.data || {};
  const pullStatus = pullData.status || pullResult.status;
  // PUL-033 contract: empty/degraded evidence must surface as a typed
  // insufficient_confidence success envelope — never context_selection_invalid.
  record("pull_no_selection_invalid", pullResult.code !== "context_selection_invalid" && pullRes?.result?.isError !== true,
    `kind=${pullResult.kind} status=${pullStatus} code=${pullResult.code} raw=${JSON.stringify(sc).slice(0, 400)}`);
  record("pull_typed_envelope", pullResult.kind === "success" && (pullStatus === "insufficient_confidence" || !!pullData.packet),
    `kind=${pullResult.kind} status=${pullStatus} degradation=${pullData.degradationReason}`);
  record("pull_semantic_retrieval", foundMarker, `foundMarker=${foundMarker} status=${pullStatus}`);
}

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
if (failed.length) process.exit(1);
