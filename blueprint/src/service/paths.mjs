// D30: daemon IPC path resolution — Unix domain socket on macOS/Linux, named
// pipe on Windows. One endpoint per user.

import { homedir, tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { join } from "node:path";

let temporaryEndpointCounter = 0;

export function daemonEndpoint({ platform = process.platform, identity = homedir() } = {}) {
  if (process.env.BLUEPRINT_DAEMON_ENDPOINT) return process.env.BLUEPRINT_DAEMON_ENDPOINT;
  if (platform === "win32") {
    const userHash = createHash("sha256").update(String(identity)).digest("hex").slice(0, 16);
    return `\\\\.\\pipe\\orthic-blueprint-${userHash}`;
  }
  return join(homedir(), ".blueprint", "blueprint.sock");
}

export function daemonPidPath() {
  return join(homedir(), ".blueprint", "daemon.pid");
}

export function temporaryDaemonEndpoint(name) {
  const suffix = `${name}-${process.pid}-${Date.now()}-${temporaryEndpointCounter++}`;
  return process.platform === "win32" ? `\\\\.\\pipe\\orthic-blueprint-${suffix}` : join(tmpdir(), `${suffix}.sock`);
}
