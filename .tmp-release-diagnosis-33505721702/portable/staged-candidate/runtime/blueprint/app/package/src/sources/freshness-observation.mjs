// Content-free repository freshness evidence owned by Blueprint.
//
// Membrane consumes this through Blueprint's resident status operation. Git
// never crosses the Blueprint boundary: only bounded commit, status, path, and
// content-digest evidence leaves this module.

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  readFileSync,
  readlinkSync,
} from "node:fs";
import { isAbsolute, relative, resolve } from "node:path";

const MAX_OVERLAY_FILES = 64;
const MAX_OVERLAY_BYTES = 64 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES = 8 * 1024 * 1024;
const GIT_TIMEOUT_MS = 2_000;
const IGNORED_OVERLAY_PREFIXES = [".agent/", ".blueprint/", "memory-mirror/"];

function digest(value) {
  return `sha256:${createHash("sha256").update(value).digest("hex")}`;
}

function git(root, args, { encoding = null, maxBuffer = MAX_GIT_OUTPUT_BYTES } = {}) {
  const result = spawnSync("git", ["-C", root, ...args], {
    encoding,
    maxBuffer,
    timeout: GIT_TIMEOUT_MS,
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.status !== 0 || result.error) return null;
  return result.stdout;
}

function gitText(root, args) {
  const output = git(root, args, { encoding: "utf8", maxBuffer: 64 * 1024 });
  const value = String(output ?? "").trim();
  return value || null;
}

function normalizePath(root, value) {
  const path = String(value ?? "").replaceAll("\\", "/");
  if (!path || isAbsolute(path) || path.split("/").includes("..")) return null;
  const absolute = resolve(root, path);
  const confined = relative(root, absolute).replaceAll("\\", "/");
  if (!confined || confined.startsWith("../") || isAbsolute(confined)) return null;
  return confined;
}

function parseStatus(root, bytes) {
  const fields = bytes.toString("utf8").split("\0");
  const entries = [];
  for (let index = 0; index < fields.length; index += 1) {
    const field = fields[index];
    if (!field) continue;
    if (field.length < 4 || field[2] !== " ") throw new Error("invalid git status record");
    const status = field.slice(0, 2);
    const path = normalizePath(root, field.slice(3));
    if (!path) throw new Error("git status path escaped repository");
    if (status[0] === "R" || status[0] === "C") index += 1;
    const absolute = resolve(root, path);
    if (existsSync(absolute) && lstatSync(absolute).isDirectory() && existsSync(resolve(absolute, ".git"))) continue;
    if (IGNORED_OVERLAY_PREFIXES.some((prefix) => path.startsWith(prefix))) continue;
    entries.push({ path, status });
  }
  return entries;
}

function sameMetadata(before, after) {
  return before.size === after.size && before.mtimeMs === after.mtimeMs && before.mode === after.mode;
}

function hashEntry(root, entry) {
  const absolute = resolve(root, entry.path);
  if (entry.status.includes("D") && !existsSync(absolute)) {
    return { ...entry, contentHash: digest("deleted"), bytes: 0, stable: true };
  }
  const before = lstatSync(absolute);
  if (before.isSymbolicLink()) {
    const target = readlinkSync(absolute);
    const after = lstatSync(absolute);
    return {
      ...entry,
      contentHash: digest(target),
      bytes: Buffer.byteLength(target),
      stable: sameMetadata(before, after),
    };
  }
  if (!before.isFile()) throw new Error("overlay entry is not a regular file");
  if (before.size > MAX_OVERLAY_BYTES) return { ...entry, contentHash: null, bytes: before.size, stable: false };
  const body = readFileSync(absolute);
  const after = lstatSync(absolute);
  return {
    ...entry,
    contentHash: digest(body),
    bytes: before.size,
    stable: sameMetadata(before, after),
  };
}

function unavailable(reason, elapsedMs) {
  return {
    available: false,
    stable: false,
    revision: null,
    commitDistance: null,
    entries: [],
    limitExceeded: false,
    stageElapsedMs: { git_status: elapsedMs },
    reason,
  };
}

export function observeRepositoryFreshness(repoRoot, { baseCommit = null } = {}) {
  const root = resolve(repoRoot);
  const started = Date.now();
  try {
    const revisionBefore = gitText(root, ["rev-parse", "HEAD"]);
    const statusBefore = git(root, ["status", "--porcelain=v1", "-z", "--untracked-files=all"]);
    if (!revisionBefore || !Buffer.isBuffer(statusBefore)) {
      return unavailable("git_status_unavailable", Date.now() - started);
    }
    const statusEntries = parseStatus(root, statusBefore);
    if (statusEntries.length > MAX_OVERLAY_FILES) {
      return {
        available: true,
        stable: false,
        revision: revisionBefore,
        commitDistance: null,
        entries: [],
        limitExceeded: true,
        stageElapsedMs: { git_status: Date.now() - started },
        reason: "overlay_file_limit_exceeded",
      };
    }

    let totalBytes = 0;
    let stable = true;
    const entries = statusEntries.map((entry) => {
      const hashed = hashEntry(root, entry);
      totalBytes += hashed.bytes;
      stable &&= hashed.stable;
      return { path: hashed.path, status: hashed.status, contentHash: hashed.contentHash };
    });
    if (totalBytes > MAX_OVERLAY_BYTES || entries.some((entry) => !entry.contentHash)) {
      return {
        available: true,
        stable: false,
        revision: revisionBefore,
        commitDistance: null,
        entries: [],
        limitExceeded: true,
        stageElapsedMs: { git_status: Date.now() - started },
        reason: "overlay_byte_limit_exceeded",
      };
    }

    const statusAfter = git(root, ["status", "--porcelain=v1", "-z", "--untracked-files=all"]);
    const revisionAfter = gitText(root, ["rev-parse", "HEAD"]);
    if (!Buffer.isBuffer(statusAfter) || !revisionAfter) {
      return unavailable("git_status_unavailable", Date.now() - started);
    }
    stable &&= statusBefore.equals(statusAfter) && revisionBefore === revisionAfter;
    entries.sort((left, right) => left.path.localeCompare(right.path) || left.status.localeCompare(right.status));

    let commitDistance = null;
    if (baseCommit && revisionAfter === baseCommit) {
      commitDistance = 0;
    } else if (baseCommit) {
      const distance = gitText(root, ["rev-list", "--count", `${baseCommit}..${revisionAfter}`]);
      if (/^\d+$/.test(distance ?? "")) commitDistance = Number(distance);
    }
    return {
      available: true,
      stable,
      revision: revisionAfter,
      commitDistance,
      entries,
      limitExceeded: false,
      stageElapsedMs: { git_status: Date.now() - started },
      reason: null,
    };
  } catch {
    return unavailable("overlay_observation_failed", Date.now() - started);
  }
}

export const _internals = { parseStatus, normalizePath };
