// D15: install the resident watcher as a user-scoped OS service. LaunchAgent
// on macOS, systemd --user on Linux, per-user scheduled task on Windows. The
// service runs the watcher in foreground mode; the OS service manager owns
// restart — no double-daemonize.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const TEMPLATE_DIR = join(SCRIPT_DIR, "templates");
const WATCH_SCRIPT = join(SCRIPT_DIR, "..", "scripts", "cortex-watch.mjs");

export function installService({ root = process.cwd(), logDir = null, dryRun = false } = {}) {
  const node = process.execPath;
  const logs = logDir ?? join(homedir(), ".cortex", "logs");
  mkdirSync(logs, { recursive: true });
  const watchScript = resolve(WATCH_SCRIPT);
  const template = join(TEMPLATE_DIR, templateName());
  if (!existsSync(template)) throw new Error(`no service template for ${process.platform}`);
  const body = readFileSync(template, "utf8")
    .replaceAll("__NODE__", node)
    .replaceAll("__WATCH_SCRIPT__", watchScript)
    .replaceAll("__LOG_DIR__", logs);
  const target = serviceTarget();
  if (dryRun) return { platform: process.platform, target, body };
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, body);
  if (process.platform === "darwin") {
    execFileSync("launchctl", ["load", target]);
  } else if (process.platform === "linux") {
    execFileSync("systemctl", ["--user", "daemon-reload"]);
    execFileSync("systemctl", ["--user", "enable", "--now", "cortex.service"]);
  } else {
    execFileSync("schtasks", ["/Create", "/F", "/TN", "OrthicCortex", "/XML", target]);
  }
  return { platform: process.platform, target, installed: true };
}

export function serviceTarget() {
  if (process.platform === "darwin") return join(homedir(), "Library", "LaunchAgents", "io.orthic.cortex.plist");
  if (process.platform === "linux") return join(homedir(), ".config", "systemd", "user", "cortex.service");
  return join(homedir(), "AppData", "Local", "Orthic", "Cortex", "cortex-task.xml");
}

export function serviceControlPlan(action, { platform = process.platform, target = serviceTarget() } = {}) {
  if (!new Set(["start", "stop", "restart"]).has(action)) throw new Error("service_action_invalid");
  if (platform === "darwin") {
    const start = { command: "launchctl", args: ["load", target] };
    const stop = { command: "launchctl", args: ["unload", target] };
    return action === "restart" ? [stop, start] : [action === "start" ? start : stop];
  }
  if (platform === "linux") {
    const verb = action === "restart" ? "restart" : action;
    return [{ command: "systemctl", args: ["--user", verb, "cortex.service"] }];
  }
  const start = { command: "schtasks", args: ["/Run", "/TN", "OrthicCortex"] };
  const stop = { command: "schtasks", args: ["/End", "/TN", "OrthicCortex"] };
  return action === "restart" ? [stop, start] : [action === "start" ? start : stop];
}

export function controlService(action, { execute = execFileSync } = {}) {
  const plan = serviceControlPlan(action);
  for (const step of plan) execute(step.command, step.args, { stdio: "ignore" });
  return { schemaVersion: 1, platform: process.platform, subcommand: action, ok: true };
}

function templateName() {
  if (process.platform === "darwin") return "io.orthic.cortex.plist";
  if (process.platform === "linux") return "cortex.service";
  return "cortex-task.xml";
}
