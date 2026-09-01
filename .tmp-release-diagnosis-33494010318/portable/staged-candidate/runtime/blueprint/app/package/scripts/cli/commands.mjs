// D05: facade command dispatch. Read commands route through the shared
// application service; write/build paths stay in the main CLI until D30.
// Graph subcommands remain as aliases.

import { createBlueprintApplicationService } from "../../src/lib/application/service.mjs";
import { RootRegistry } from "../../src/lib/application/root-registry.mjs";
import { applyInitPlan, uninstallInit } from "../../src/lib/init/apply.mjs";
import { buildInitPlan } from "../../src/lib/init/plan.mjs";
import { recoverPendingUpdate } from "../../src/lib/update/apply.mjs";
import { timingSafeEqual } from "node:crypto";
import { join, resolve } from "node:path";
import { createDaemonServer } from "../../src/service/server.mjs";
import { readWatchConfig } from "../../watchman/supervisor.mjs";
import { startBlueprintMcpServer } from "../blueprint-mcp.mjs";
import { EXIT, parseArgs } from "./args.mjs";
import { machineError, printResult, renderArchitecture, renderDocTruth, renderExpand, renderImpact, renderSearch, renderStatus } from "./render.mjs";

function serviceFor(args) {
  return createBlueprintApplicationService({
    outDir: String(args.out ?? ".agent"),
    rootRegistry: new RootRegistry(readWatchConfig().repos),
    allowEmbeddedRoot: false,
  });
}

const HUB_PARENT_PID_ENV = "MEMBRANE_HUB_PARENT_PID";
const HUB_LAUNCH_TOKEN_ENV = "MEMBRANE_HUB_LAUNCH_TOKEN";
const HUB_LAUNCH_HANDSHAKE_TIMEOUT_MS = 2000;
const WATCHER_DRAIN_TIMEOUT_MS = 2000;

function launchTokenMatches(received, expected) {
  const left = Buffer.from(String(received).trim());
  const right = Buffer.from(String(expected));
  return left.length === right.length && timingSafeEqual(left, right);
}

function readHubLaunchToken(expected) {
  return new Promise((resolve) => {
    const stdin = process.stdin;
    let buffer = "";
    let settled = false;
    let timer;
    const cleanup = () => {
      clearTimeout(timer);
      stdin.off("data", onData);
      stdin.off("end", onEnd);
      stdin.off("error", onError);
    };
    const finish = (ok) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(ok);
    };
    const onData = (chunk) => {
      buffer += chunk.toString();
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      finish(launchTokenMatches(buffer.slice(0, newline), expected));
    };
    const onEnd = () => finish(false);
    const onError = () => finish(false);
    stdin.setEncoding("utf8");
    stdin.on("data", onData);
    stdin.once("end", onEnd);
    stdin.once("error", onError);
    stdin.resume();
    timer = setTimeout(() => finish(false), HUB_LAUNCH_HANDSHAKE_TIMEOUT_MS);
  });
}

async function authorizeResidentLaunch() {
  if (process.env.MEMBRANE_HUB_CHILD !== "1") return { ok: false, code: "hub_inactive" };
  const parentPid = Number(process.env[HUB_PARENT_PID_ENV]);
  const expected = process.env[HUB_LAUNCH_TOKEN_ENV];
  if (!Number.isSafeInteger(parentPid) || parentPid <= 0 || parentPid !== process.ppid) {
    return { ok: false, code: "hub_inactive" };
  }
  if (!/^[0-9a-f]{64}$/.test(expected ?? "")) return { ok: false, code: "hub_inactive" };
  if (!(await readHubLaunchToken(expected))) return { ok: false, code: "hub_inactive" };
  return { ok: true };
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
      else console.log(`Open Blueprint Explorer: ${explorer.url}`);
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
        printResult(machineError("os_registration_forbidden", "OS service control forbidden per D-S03 — use blueprint service run; Hub owns lifecycle"), args, { stderr: true });
        return EXIT.INTERNAL;
      }
      if (subcommand === "run") {
        // D-S04 headless carve-out — only an active Hub may create resident
        // Blueprint service/watcher processes. Direct Blueprint consumers use
        // BlueprintClient's bounded one-shot path instead.
        const launch = await authorizeResidentLaunch();
        if (!launch.ok) {
          const payload = machineError("hub_inactive", "Blueprint service residency requires an active Membrane Hub; use a direct bounded Blueprint operation");
          printResult(payload, args);
          printResult(payload, args, { stderr: true });
          return EXIT.INTERNAL;
        }
        const { spawn } = await import("node:child_process");
        const { startSnapshotServer } = await import("../../src/lib/snapshot.mjs");
        let endpoint;
        let daemon;
        let daemonAddress;
        let watcher;
        let watcherError = "";
        let watcherOutput = "";
        let watcherSpawnError = null;
        try {
          endpoint = await startSnapshotServer({ root, authToken: process.env.BLUEPRINT_SNAPSHOT_TOKEN });
          daemon = createDaemonServer({ registryEntries: readWatchConfig().repos });
          daemonAddress = await daemon.listen();
          // Blueprint, not any peer, owns its watcher. Keep it attached to this
          // foreground service so Hub termination also stops the watcher.
          const watcherScript = resolve(import.meta.dirname, "../blueprint-watch.mjs");
          watcher = spawn(process.execPath, [watcherScript, "start"], {
            cwd: root,
            env: {
              ...process.env,
              BLUEPRINT_SERVICE_CHILD: "1",
              MEMBRANE_BLUEPRINT_PARENT_PID: String(process.pid),
              MEMBRANE_BLUEPRINT_LAUNCH_TOKEN: process.env[HUB_LAUNCH_TOKEN_ENV],
            },
            stdio: ["pipe", "pipe", "pipe"],
            windowsHide: true,
          });
          watcher.stdin.write(`${process.env[HUB_LAUNCH_TOKEN_ENV]}\n`);
          watcher.stdout?.setEncoding("utf8");
          watcher.stdout?.on("data", (chunk) => { watcherOutput += chunk; });
          watcher.stderr?.setEncoding("utf8");
          watcher.stderr?.on("data", (chunk) => { watcherError += chunk; });
          watcher.once("error", (error) => { watcherSpawnError = error; });
          await new Promise((resolve) => setTimeout(resolve, 150));
          if (watcherSpawnError || watcher.exitCode !== null) {
            const detail = `${watcherOutput}\n${watcherError}`.toLowerCase();
            const lifecycle = ["resident_owner_active", "hub_inactive", "not_configured", "stale", "transport_unavailable"]
              .find((code) => detail.includes(code));
            const error = new Error(lifecycle ?? "blueprint_watcher_unavailable");
            error.code = lifecycle ?? (watcherSpawnError?.code ?? "blueprint_watcher_unavailable");
            throw error;
          }
        } catch (error) {
          watcher?.kill("SIGTERM");
          await daemon?.close().catch(() => {});
          await endpoint?.close().catch(() => {});
          const code = error?.code ?? "snapshot_server_failed";
          const payload = machineError(code, String(error?.message ?? error));
          printResult(payload, args);
          printResult(payload, args, { stderr: true });
          return EXIT.INTERNAL;
        }
        const payload = { schemaVersion: 1, state: "running", mode: "foreground", owner: "hub", pid: process.pid, watcherPid: watcher.pid, serviceStart: [process.execPath, "scripts/blueprint.mjs", "service", "run"], daemonEndpoint: daemonAddress.endpoint, statusEndpoint: { host: endpoint.host, port: endpoint.port, authHeader: endpoint.authHeader } };
        console.log(JSON.stringify(payload));
        // Keep event loop alive — signal listeners alone don't ref the loop, so Node would exit with 13 (unsettled top-level await)
        const keepAlive = setInterval(() => {}, 1000);
        await new Promise((resolve) => {
          let shuttingDown = false;
          const stopWatcher = async () => {
            if (!watcher || watcher.exitCode !== null) return;
            watcher.kill("SIGTERM");
            const exited = await Promise.race([
              new Promise((settle) => watcher.once("exit", () => settle(true))),
              new Promise((settle) => setTimeout(() => settle(false), WATCHER_DRAIN_TIMEOUT_MS)),
            ]);
            if (!exited && watcher.exitCode === null) watcher.kill("SIGKILL");
          };
          const shutdown = (failure = false) => {
            if (shuttingDown) return;
            shuttingDown = true;
            if (failure) process.exitCode = EXIT.INTERNAL;
            clearInterval(keepAlive);
            Promise.allSettled([stopWatcher(), daemon.close(), endpoint.close()]).finally(resolve);
          };
          const watcherExit = (code) => {
            if (shuttingDown) return;
            // A resident watcher disappearing makes service readiness false;
            // let Hub observe a typed failure instead of a false running loop.
            if (code !== 0) process.exitCode = EXIT.INTERNAL;
            shutdown(true);
          };
          watcher.once("exit", watcherExit);
          if (watcher.exitCode !== null) watcherExit(watcher.exitCode);
          process.once("SIGTERM", () => shutdown(false));
          process.once("SIGINT", () => shutdown(false));
          if (process.env.MEMBRANE_HUB_CHILD === "1") {
            process.stdin.resume();
            process.stdin.once("end", () => shutdown(false));
            process.stdin.once("close", () => shutdown(false));
            process.stdin.once("error", () => shutdown(false));
          }
        });
        return EXIT.OK;
      }
      if (subcommand === "logs") {
        const { homedir } = await import("node:os");
        const { readFileSync, existsSync } = await import("node:fs");
        const { join } = await import("node:path");
        const logPath = join(homedir(), ".blueprint", "logs", "service.log");
        printResult({ path: logPath, tail: existsSync(logPath) ? readFileSync(logPath, "utf8").split(/\r?\n/).slice(-40).join("\n") : "" }, args);
        return EXIT.OK;
      }
      printResult(machineError("usage", `blueprint service ${subcommand} is not a known subcommand`), args, { stderr: true });
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
          reason: !enabled ? (offline ? "offline" : process.env.BLUEPRINT_NO_UPDATE_CHECK === "1" ? "disabled_by_env" : "disabled") : "enabled",
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
        printResult({ schemaVersion: 1, owner, action: "require-signed-manifest", reason: "GitHub Release updates require a signed manifest and matching checksum" }, args);
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
      printResult(machineError("usage", `blueprint update ${subcommand} is not a known subcommand`), args, { stderr: true });
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
      const rulesPath = join(root, "blueprint.rules.yml");
      if (!existsSync(rulesPath)) {
        printResult(machineError("rules_missing", "blueprint.rules.yml not found in repository root"), args, { stderr: true });
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
      printResult(machineError("usage", `blueprint rules ${subcommand} is not a known subcommand`), args, { stderr: true });
      return EXIT.USAGE;
    }
    case "mcp":
      // CX-B1: `blueprint mcp serve --root <repo>` starts the stdio server from
      // scripts/blueprint-mcp.mjs in-process; the CLI process becomes the server.
      {
        const subcommand = String(args._[0] ?? args.subcommand ?? "");
        if (subcommand === "serve") {
          await startBlueprintMcpServer({ root: args.root ?? root });
          return EXIT.OK;
        }
        printResult(machineError("usage", `blueprint mcp ${subcommand} is not a known subcommand; use blueprint mcp serve --root <repo>`), args, { stderr: true });
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
