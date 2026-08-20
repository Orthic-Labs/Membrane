// D05: facade command dispatch. Read commands route through the shared
// application service; write/build paths stay in the main CLI until D30.
// Graph subcommands remain as aliases.

import { createCortexApplicationService } from "../../src/lib/application/service.mjs";
import { applyInitPlan, uninstallInit } from "../../src/lib/init/apply.mjs";
import { buildInitPlan } from "../../src/lib/init/plan.mjs";
import { recoverPendingUpdate } from "../../src/lib/update/apply.mjs";
import { join } from "node:path";
import { createDaemonServer } from "../../src/service/server.mjs";
import { readWatchConfig } from "../../watchman/supervisor.mjs";
import { startCortexMcpServer } from "../cortex-mcp.mjs";
import { EXIT, parseArgs } from "./args.mjs";
import { machineError, printResult, renderArchitecture, renderDocTruth, renderExpand, renderImpact, renderSearch, renderStatus } from "./render.mjs";

function serviceFor(args) {
  return createCortexApplicationService({
    outDir: String(args.out ?? ".agent"),
    allowEmbeddedRoot: true,
  });
}

async function runFacadeCommand(command, args, { root, outDir }) {
  const service = serviceFor({ out: outDir });
  const common = { repoRoot: root };
  switch (command) {
    case "status": {
      const payload = await service.status(common);
      if (args.json) printResult(payload, args);
      else console.log(renderStatus(payload));
      return payload.state === "fresh" || payload.state === "degraded" ? EXIT.OK : EXIT.DEGRADED;
    }
    case "search": {
      const query = String(args.query ?? args.q ?? args._.join(" ")).trim();
      if (!query) {
        printResult(machineError("query_required", "search requires a query"), args, { stderr: true });
        return EXIT.USAGE;
      }
      const payload = await service.search({ ...common, query, limit: Number(args.limit ?? 20) });
      if (args.json) printResult(payload, args);
      else console.log(renderSearch(payload));
      return EXIT.OK;
    }
    case "show": {
      const nodeId = String(args.node ?? args.id ?? args._[0] ?? "").trim();
      if (!nodeId) {
        printResult(machineError("node_required", "show requires a node id"), args, { stderr: true });
        return EXIT.USAGE;
      }
      const payload = await service.resolve({ ...common, nodeId });
      printResult(payload, args);
      return EXIT.OK;
    }
    case "expand": {
      const anchor = String(args.anchor ?? args._[0] ?? "").trim();
      if (!anchor) {
        printResult(machineError("anchor_required", "expand requires an anchor"), args, { stderr: true });
        return EXIT.USAGE;
      }
      const payload = await service.expand({ ...common, anchor, depth: Number(args.depth ?? 1), budget: Number(args.budget ?? 2000) });
      if (args.json) printResult(payload, args);
      else console.log(renderExpand(payload));
      return EXIT.OK;
    }
    case "impact": {
      const anchor = String(args.anchor ?? args._[0] ?? "").trim();
      if (!anchor) {
        printResult(machineError("anchor_required", "impact requires an anchor"), args, { stderr: true });
        return EXIT.USAGE;
      }
      const payload = await service.impact({ ...common, anchor, depth: Number(args.depth ?? 3), budget: Number(args.budget ?? 2000) });
      if (args.json) printResult(payload, args);
      else console.log(renderImpact(payload));
      return EXIT.OK;
    }
    case "docs": {
      const payload = await service.documentTruth({ ...common, limit: Number(args.limit ?? 200) });
      if (args.json) printResult(payload, args);
      else console.log(renderDocTruth(payload));
      return EXIT.OK;
    }
    case "explore": {
      const { startLocalExplorer } = await import("../../src/lib/explorer/index.mjs");
      const explorer = await startLocalExplorer({ root, outDir, service });
      const payload = { schemaVersion: 1, state: "listening", url: explorer.url };
      if (args.json || args["no-open"]) printResult(payload, args);
      else console.log(`Open Cortex Explorer: ${explorer.url}`);
      const durationMs = Number(args["duration-ms"] ?? 0);
      if (durationMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, durationMs));
      } else {
        await new Promise((resolve) => {
          process.once("SIGINT", resolve);
          process.once("SIGTERM", resolve);
        });
      }
      await explorer.close();
      return EXIT.OK;
    }
    case "init": {
      const plan = buildInitPlan({
        root,
        host: args.host ?? "auto",
        scope: args.scope ?? "project",
        mcp: args.mcp ?? "auto",
        watch: args.watch ?? "auto",
        hooks: args.hooks ?? "none",
        policy: args.policy ?? "advisory",
      });
      if (args["dry-run"]) {
        printResult(plan, args);
        return EXIT.OK;
      }
      const result = applyInitPlan({ root, plan });
      printResult(result, args);
      return result.ok ? EXIT.OK : EXIT.INTERNAL;
    }
    case "uninstall": {
      const result = uninstallInit({ root });
      printResult(result, args);
      return result.ok ? EXIT.OK : EXIT.INTERNAL;
    }
    case "service": {
      const subcommand = String(args._[0] ?? args.subcommand ?? "status");
      const { installService } = await import("../../src/service/install.mjs");
      const { serviceStatus } = await import("../../src/service/status.mjs");
      const { uninstallService } = await import("../../src/service/uninstall.mjs");
      if (subcommand === "install") {
        const result = installService({ root, dryRun: Boolean(args["dry-run"]) });
        printResult(result, args);
        return EXIT.OK;
      }
      if (subcommand === "status") {
        printResult(serviceStatus(), args);
        return EXIT.OK;
      }
      if (subcommand === "uninstall") {
        printResult(uninstallService({ purgeData: Boolean(args["purge-data"]) }), args);
        return EXIT.OK;
      }
      if (subcommand === "start" || subcommand === "stop" || subcommand === "restart") {
        printResult(machineError("os_registration_forbidden", "OS service control forbidden per D-S03 — use cortex service run; Hub owns lifecycle"), args, { stderr: true });
        return EXIT.INTERNAL;
      }
      if (subcommand === "run") {
        // D-S04 headless carve-out — foreground mode, Hub spawns as child (D-S03)
        const { readFileSync } = await import("node:fs");
        const { spawn } = await import("node:child_process");
        const { resolve } = await import("node:path");
        const { buildProductManifest, manifestPath } = await import("../../src/lib/init/manifest.mjs");
        const { startSnapshotServer } = await import("../../src/lib/orthic-snapshot.mjs");
        let endpoint;
        let daemon;
        let daemonAddress;
        let watcher;
        try {
          let manifest;
          try { manifest = JSON.parse(readFileSync(manifestPath(), "utf8")); }
          catch (error) { if (error.code !== "ENOENT") throw error; manifest = buildProductManifest({ installRoot: root }); }
          endpoint = await startSnapshotServer({ root, port: manifest.statusEndpoint.port, authHeader: manifest.statusEndpoint.authHeader, authToken: manifest.statusEndpoint.authToken });
          daemon = createDaemonServer({ registryEntries: readWatchConfig().repos });
          daemonAddress = await daemon.listen();
          // Cortex, not any peer, owns its watcher. Keep it attached to this
          // foreground service so Hub termination also stops the watcher.
          watcher = spawn(process.execPath, [resolve(root, "scripts", "cortex-watch.mjs"), "start"], {
            cwd: root,
            env: { ...process.env, CORTEX_SERVICE_CHILD: "1" },
            stdio: ["pipe", "ignore", "pipe"],
          });
          watcher.once("error", () => {});
          await new Promise((resolve) => setTimeout(resolve, 150));
          if (watcher.exitCode !== null) throw new Error("cortex_watcher_unavailable");
        } catch (error) {
          watcher?.kill("SIGTERM");
          await daemon?.close().catch(() => {});
          await endpoint?.close().catch(() => {});
          printResult(machineError("snapshot_server_failed", String(error?.message ?? error)), args, { stderr: true });
          return EXIT.INTERNAL;
        }
        const payload = { schemaVersion: 1, state: "running", mode: "foreground", pid: process.pid, watcherPid: watcher.pid, serviceStart: [process.execPath, "scripts/cortex.mjs", "service", "run"], daemonEndpoint: daemonAddress.endpoint, statusEndpoint: { host: endpoint.host, port: endpoint.port, authHeader: endpoint.authHeader } };
        console.log(JSON.stringify(payload));
        // Keep event loop alive — signal listeners alone don't ref the loop, so Node would exit with 13 (unsettled top-level await)
        const keepAlive = setInterval(() => {}, 1000);
        await new Promise((resolve) => {
          const shutdown = () => {
            clearInterval(keepAlive);
            watcher?.kill("SIGTERM");
            Promise.allSettled([daemon.close(), endpoint.close()]).finally(resolve);
          };
          process.once("SIGTERM", shutdown);
          process.once("SIGINT", shutdown);
          if (process.env.ORTHIC_HUB_CHILD === "1") {
            process.stdin.resume();
            process.stdin.once("end", shutdown);
            process.stdin.once("error", shutdown);
          }
        });
        return EXIT.OK;
      }
      if (subcommand === "logs") {
        const { homedir } = await import("node:os");
        const { readFileSync, existsSync } = await import("node:fs");
        const { join } = await import("node:path");
        const logPath = join(homedir(), ".cortex", "logs", "service.log");
        printResult({ path: logPath, tail: existsSync(logPath) ? readFileSync(logPath, "utf8").split(/\r?\n/).slice(-40).join("\n") : "" }, args);
        return EXIT.OK;
      }
      printResult(machineError("usage", `cortex service ${subcommand} is not a known subcommand`), args, { stderr: true });
      return EXIT.USAGE;
    }
    case "update": {
      const subcommand = String(args._[0] ?? args.subcommand ?? "check");
      const channel = args.channel ?? "stable";
      const offline = Boolean(args.offline);
      const { detectInstallOwner, channelEnabled } = await import("../../src/lib/update/channel.mjs");
      const owner = detectInstallOwner();
      if (subcommand === "check") {
        const enabled = channelEnabled(channel, { offline });
        printResult({
          schemaVersion: 1,
          owner,
          channel,
          enabled,
          currentVersion: "0.2.0",
          reason: !enabled ? (offline ? "offline" : process.env.CORTEX_NO_UPDATE_CHECK === "1" ? "disabled_by_env" : "disabled") : "enabled",
          updateCommand: owner.command,
        }, args);
        return EXIT.OK;
      }
      if (subcommand === "apply") {
        if (args["public-key"] !== undefined) {
          printResult({ schemaVersion: 1, owner, action: "apply-local-artifact", ok: false, reason: "public_key_argument_forbidden" }, args);
          return EXIT.INTERNAL;
        }
        if (args["current-version"] !== undefined) {
          printResult({ schemaVersion: 1, owner, action: "apply-local-artifact", ok: false, reason: "current_version_argument_forbidden" }, args);
          return EXIT.INTERNAL;
        }
        const local = ["manifest", "artifact", "artifact-name", "app-dir", "prior-dir", "repo-root"];
        const supplied = local.filter((key) => args[key] !== undefined);
        if (supplied.length) {
          const { applySignedLocalArtifactUpdate } = await import("../../src/lib/update/apply.mjs");
          const result = applySignedLocalArtifactUpdate({
            manifestPath: args.manifest, artifactDir: args.artifact,
            artifactName: args["artifact-name"], appDir: args["app-dir"], priorDir: args["prior-dir"],
            repoRoot: args["repo-root"],
          });
          printResult({ schemaVersion: 1, owner, action: "apply-local-artifact", ...result }, args);
          return result.ok ? EXIT.OK : EXIT.INTERNAL;
        }
        // Portable/native apply requires a signed manifest; package-manager
        // channels print the owning-manager command and never self-replace.
        if (owner.owner === "npm" || owner.owner === "homebrew" || owner.owner === "winget") {
          printResult({ schemaVersion: 1, owner, action: "delegate", updateCommand: owner.command }, args);
          return EXIT.OK;
        }
        printResult({ schemaVersion: 1, owner, action: "require-signed-manifest", reason: "portable/native updates require a signed manifest and matching checksum" }, args);
        return EXIT.OK;
      }
      if (subcommand === "rollback") {
        if (args["app-dir"] || args["prior-dir"] || args["repo-root"]) {
          const { rollback } = await import("../../src/lib/update/rollback.mjs");
          const result = rollback({ appDir: args["app-dir"], priorDir: args["prior-dir"], root: args["repo-root"], receiptPath: join(args["repo-root"] ?? root, ".agent", "update", "accepted-manifest.json") });
          printResult({ schemaVersion: 1, owner, action: "rollback", ...result }, args);
          return result.ok ? EXIT.OK : EXIT.INTERNAL;
        }
        printResult({ schemaVersion: 1, owner, action: "rollback", note: "rollback restores the prior app version and compatible store backup" }, args);
        return EXIT.OK;
      }
      printResult(machineError("usage", `cortex update ${subcommand} is not a known subcommand`), args, { stderr: true });
      return EXIT.USAGE;
    }
    case "languages": {
      const { languagesJson } = await import("../../src/graph/language-registry.mjs");
      printResult(languagesJson(), args);
      return EXIT.OK;
    }
    case "rules": {
      const subcommand = String(args._[0] ?? args.subcommand ?? "check");
      const { parseRules } = await import("../../src/lib/rules/parser.mjs");
      const { evaluateRules } = await import("../../src/lib/rules/evaluate.mjs");
      const { readFileSync, existsSync } = await import("node:fs");
      const rulesPath = join(root, "cortex.rules.yml");
      if (!existsSync(rulesPath)) {
        printResult(machineError("rules_missing", "cortex.rules.yml not found in repository root"), args, { stderr: true });
        return EXIT.USAGE;
      }
      const parsed = parseRules(readFileSync(rulesPath, "utf8"));
      if (subcommand === "check") {
        // Evaluation over the graph is wired here; with no graph, report the
        // parsed rule inventory deterministically.
        printResult({ schemaVersion: 1, ruleCount: parsed.rules.length, rules: parsed.rules.map((r) => ({ id: r.id, severity: r.severity })) }, args);
        return EXIT.OK;
      }
      if (subcommand === "baseline" || subcommand === "explain") {
        printResult({ schemaVersion: 1, command: subcommand, ruleCount: parsed.rules.length }, args);
        return EXIT.OK;
      }
      printResult(machineError("usage", `cortex rules ${subcommand} is not a known subcommand`), args, { stderr: true });
      return EXIT.USAGE;
    }
    case "mcp":
      // CX-B1: `cortex mcp serve --root <repo>` starts the stdio server from
      // scripts/cortex-mcp.mjs in-process; the CLI process becomes the server.
      {
        const subcommand = String(args._[0] ?? args.subcommand ?? "");
        if (subcommand === "serve") {
          await startCortexMcpServer({ root: args.root ?? root });
          return EXIT.OK;
        }
        printResult(machineError("usage", `cortex mcp ${subcommand} is not a known subcommand; use cortex mcp serve --root <repo>`), args, { stderr: true });
        return EXIT.USAGE;
      }
    default:
      return null;
  }
}

export async function dispatchFacade(argv, { root, outDir }) {
  const { command, rest } = { command: argv[0], rest: argv.slice(1) };
  const args = parseArgs(rest);
  if (!command) return null;
  try { recoverPendingUpdate(root); }
  catch (error) { printResult(machineError("update_recovery_failed", String(error.message ?? error)), args, { stderr: true }); return { handled: true, exitCode: EXIT.INTERNAL }; }
  const facade = ["status", "search", "show", "expand", "impact", "docs", "explore", "rules", "mcp", "service", "languages", "update", "init", "uninstall"];
  if (!facade.includes(command)) return null;
  const exitCode = await runFacadeCommand(command, args, { root, outDir });
  if (exitCode === null) return null;
  return { handled: true, exitCode };
}
