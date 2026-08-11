// D11: doctor diagnostics core, extracted from the CLI. Preserves the
// existing reason codes and typed state ladder
// (ready|degraded|stale|broken|corrupt|missing).

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { graphStatus, graphCapabilities } from "../../graph/static-provider.mjs";

// CX-B2: launchability liveness window. A correct MCP config stays alive this
// long (it is waiting on the stdio transport); the broken config (missing
// --root) exits after its cold module startup with code 1.
const MCP_LIVENESS_MS = 4000;

// Deviation recorded in .audit/cx-b2/step-3-probe.log: stdio:"ignore" connects
// stdin to NUL/EOF and the server exits code 0 before the window, while
// stdio:["pipe","ignore","ignore"] stays alive (parent keeps stdin open). The
// probe therefore keeps child stdin open and ignores stdout/stderr only.
// collectDoctorDiagnostics is synchronous, so the probe is a stdlib-only
// subprocess: it spawns command+args with open stdin, waits the window,
// determines alive/exit code, kills a healthy child, catches spawn errors, and
// prints typed JSON.
const MCP_LIVENESS_PROBE = [
  "const fs=require('node:fs');const{spawn}=require('node:child_process');let settled=false;",
  "const finish=r=>{if(settled)return;settled=true;fs.writeSync(1,JSON.stringify(r)+'\\n');process.exit(0);};",
  "let payload;try{payload=JSON.parse(process.argv[1]);}catch(e){finish({alive:false,code:null,spawnError:'invalid probe payload: '+String(e&&e.message||e)});}",
  "let child;try{child=spawn(payload.command,Array.isArray(payload.args)?payload.args:[],{stdio:['pipe','ignore','ignore'],windowsHide:true});}",
  "catch(e){finish({alive:false,code:null,spawnError:String(e&&e.message||e)});}",
  "let killed=false;child.on('error',e=>finish({alive:false,code:null,spawnError:String(e&&e.message||e)}));",
  "child.on('exit',(code,signal)=>{if(killed)return;finish({alive:false,code:code==null?null:code,signal:signal||null,spawnError:null});});",
  `setTimeout(()=>{killed=true;try{child.kill();}catch(e){}setTimeout(()=>finish({alive:true,code:null,spawnError:null}),100);},${MCP_LIVENESS_MS});`,
].join("\n");

export function readJson(path, fallback) {
  if (!existsSync(path)) return fallback;
  try { return JSON.parse(readFileSync(path, "utf8")); } catch { return fallback; }
}

function probeMcpLaunchability(command, args) {
  let result;
  try {
    result = spawnSync(process.execPath, ["-e", MCP_LIVENESS_PROBE, JSON.stringify({ command, args })], {
      encoding: "utf8",
      timeout: MCP_LIVENESS_MS + 5000,
      windowsHide: true,
    });
  } catch (error) {
    return { spawnError: String(error?.message ?? error) };
  }
  if (result.error || result.status !== 0) return { spawnError: `${result.error?.message ?? ""} ${String(result.stderr ?? "").trim()}`.trim() || `probe exited with status ${result.status}` };
  try { return JSON.parse(result.stdout); } catch { return { spawnError: "liveness probe returned no parseable result" }; }
}

// CX-B2: mcp_config_launchable. Applies only when root/.mcp.json parses and
// has mcpServers.cortex; returns null otherwise. Pass when the recorded
// command+args is still alive after the liveness window; fail when it exits
// early (evidence carries the exact command, JSON args, and exit code); spawn
// or malformed-config problems become typed warnings and never throw.
function mcpConfigLaunchable(root) {
  const config = readJson(join(root, ".mcp.json"), null);
  const entry = config?.mcpServers?.cortex;
  if (!entry || typeof entry !== "object") return null;
  const command = entry.command;
  const args = Array.isArray(entry.args) ? entry.args : [];
  if (typeof command !== "string" || command.length === 0) {
    return { status: "warning", message: "MCP server config for cortex is invalid: command is missing or not a string, so launchability cannot be checked." };
  }
  let probe;
  try { probe = probeMcpLaunchability(command, args); } catch (error) {
    return { status: "warning", message: `MCP launchability probe could not run: ${String(error?.message ?? error)}` };
  }
  if (probe.spawnError) return { status: "warning", message: `MCP server config for cortex could not be spawn-checked: ${probe.spawnError}` };
  if (probe.alive) return { status: "pass", command, args, message: `MCP server config for cortex stayed alive through the ${MCP_LIVENESS_MS} ms liveness window.` };
  return {
    status: "fail",
    command,
    args,
    exitCode: probe.code,
    message: `MCP server config for cortex is not launchable: spawned command "${command}" with args ${JSON.stringify(args)} and it exited early with exit code ${probe.code} before the ${MCP_LIVENESS_MS} ms liveness window; expected the server to stay alive waiting on stdio.`,
  };
}

export function collectDoctorDiagnostics(root, outDir = ".agent", { full = false } = {}) {
  const startedAt = new Date().toISOString();
  const mapPath = join(root, outDir, "map.json");
  const stalePath = join(root, outDir, "stale.json");
  if (!existsSync(mapPath)) {
    return {
      schemaVersion: 1,
      state: "missing",
      generatedAt: startedAt,
      artifacts: { map: `${outDir}/map.json`, graph: `${outDir}/graph/graph.db` },
      errors: [`${outDir}/map.json missing; run build first`],
      warnings: [],
      reasons: [{ code: "missing_map", severity: "blocker", message: "Cortex map.json is not present; planner cannot retrieve candidates." }],
      capabilities: graphCapabilities(),
    };
  }
  let map;
  let stale;
  try {
    map = readJson(mapPath, null);
    stale = readJson(stalePath, { missingReferences: [] });
  } catch (error) {
    return {
      schemaVersion: 1,
      state: "corrupt",
      generatedAt: startedAt,
      artifacts: { map: `${outDir}/map.json`, graph: `${outDir}/graph/graph.db` },
      errors: [String(error?.message ?? error)],
      warnings: [],
      reasons: [{ code: "corrupt_map", severity: "blocker", message: "Cortex map.json could not be parsed." }],
      capabilities: graphCapabilities(),
    };
  }
  const errors = [];
  const warnings = [];
  const reasons = [];
  const ids = new Set();
  for (const node of map.nodes) {
    if (ids.has(node.id)) errors.push(`duplicate node id: ${node.id}`);
    ids.add(node.id);
  }
  if (ids.size !== map.nodes.length) {
    reasons.push({ code: "duplicate_node_ids", severity: "blocker", message: `graph contains ${map.nodes.length - ids.size} duplicate node id(s); discovery must enforce collision-safe IDs.` });
  }
  for (const edge of map.edges) {
    if (!ids.has(edge.from)) errors.push(`edge missing from node: ${edge.from}`);
    if (!ids.has(edge.to)) errors.push(`edge missing to node: ${edge.to}`);
  }
  for (const warning of stale.missingReferences.slice(0, 10)) {
    warnings.push(`${warning.source} mentions missing ${warning.path}`);
  }
  if (stale.missingReferences.length > 0) {
    reasons.push({ code: "missing_references", severity: "warning", count: stale.missingReferences.length, message: "documents reference paths that no longer exist on disk" });
  }
  const mcp = mcpConfigLaunchable(root);
  if (mcp) {
    if (mcp.status === "pass") reasons.push({ code: "mcp_config_launchable", severity: "info", status: "pass", command: mcp.command, args: mcp.args, message: mcp.message });
    else if (mcp.status === "warning") { warnings.push(mcp.message); reasons.push({ code: "mcp_config_launchable", severity: "warning", message: mcp.message }); }
    else { errors.push(mcp.message); reasons.push({ code: "mcp_config_launchable", severity: "blocker", command: mcp.command, args: mcp.args, exitCode: mcp.exitCode, message: mcp.message }); }
  }
  const graph = graphStatus(root, outDir);
  if (graph.state === "stale") {
    warnings.push(graph.providerMismatch ? "graph manifest was built by an older Cortex provider and must be rebuilt" : "graph manifest source hash is stale relative to current files");
    reasons.push({ code: "stale_graph", severity: "blocker", providerMismatch: Boolean(graph.providerMismatch), message: graph.providerMismatch ? "graph provider version does not match the installed Cortex extractor; rebuild required." : "graph manifest sourceHash does not match current source tree; rebuild required." });
  }
  return {
    schemaVersion: 1,
    state: errors.length ? "broken" : "degraded",
    generatedAt: startedAt,
    artifacts: { map: `${outDir}/map.json`, graph: `${outDir}/graph/graph.db` },
    errors,
    warnings,
    reasons,
    capabilities: graphCapabilities(),
    graph,
    completion: full ? { checked: true } : null,
  };
}
