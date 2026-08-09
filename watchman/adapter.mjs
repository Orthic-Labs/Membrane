import ParcelWatcher from "@parcel/watcher";
import { readdirSync, realpathSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { randomUUID } from "node:crypto";
import { SCAN_EXCLUSIONS } from "../graph/static-provider.mjs";

const EVENT_TYPES = new Set(["create", "update", "delete"]);
const PROBE_PREFIX = "cortex-watch-probe-";
const PROBE_DEADLINE_MS = 30_000;
const PROBE_CADENCE_MS = 1_000;

function cancelled() { return Object.assign(new Error("request cancelled"), { code: "request_cancelled" }); }

function canonicalRoot(value) {
  const root = resolve(value);
  try { return realpathSync(root); } catch { return root; }
}

function canonicalAbsolute(value) {
  const absolute = resolve(value);
  try { return realpathSync(absolute); }
  catch {
    const parent = dirname(absolute);
    return parent === absolute ? absolute : resolve(canonicalAbsolute(parent), basename(absolute));
  }
}

function normalizePath(root, value) {
  return relative(root, canonicalAbsolute(resolve(root, String(value)))).replaceAll("\\", "/");
}

export async function waitForNativeProbe(readyPromise, timeoutMs = PROBE_CADENCE_MS, signal) {
  let probeTimer;
  let onAbort;
  try {
    await Promise.race([
      readyPromise,
      new Promise((_, reject) => {
        probeTimer = setTimeout(() => {
          const error = new Error("native watcher readiness probe timed out");
          error.code = "watch_probe_timeout";
          reject(error);
        }, timeoutMs);
        probeTimer.unref?.();
      }),
      new Promise((_, reject) => {
        onAbort = () => reject(cancelled());
        if (signal?.aborted) onAbort();
        else signal?.addEventListener("abort", onAbort, { once: true });
      }),
    ]);
  } finally {
    clearTimeout(probeTimer);
    signal?.removeEventListener("abort", onAbort);
  }
}

// @parcel/watcher's `ignore` option (typed GlobPattern in its own .d.ts) does NOT
// glob-match in the installed native build (2.6.0, fs-events backend) — confirmed
// empirically 2026-08-03: `node_modules/**` and `**/node_modules/**` pass every
// event through unfiltered. What actually excludes a subtree is an EXACT literal
// path relative to the watched root: a bare single-segment name (e.g.
// "node_modules") excludes that name only where it sits directly under the
// watched root, and a multi-segment relative path (e.g. "child-repo" or
// "nested/child-repo") excludes that exact target at any depth. Neither form is
// a glob; do not reintroduce `**` or trailing `/**` here, it silently no-ops.
// `extra` therefore carries exact relative paths — the caller (repo-actor via
// the supervisor) resolves them from its own root before passing them in.
const EXCLUDED_NAMES = Object.freeze([".git", "node_modules", ".agent", ...SCAN_EXCLUSIONS]);

// The scanner prunes any directory whose NAME is excluded, at any depth. The
// watcher cannot: per the note above, a bare name only excludes that name
// directly under the watched root. So `target` excluded `<root>/target` and
// silently watched `engine/target` — coderight's entire Rust build output.
// Measured 2026-08-03: 395 of coderight's last 400 unapplied events were build
// churn under `engine/`, and the resulting load starved the other actors.
//
// Bridge the two by resolving names into the exact relative paths the watcher
// does honour, walking once at subscribe time and never descending into a
// directory already excluded — so the walk costs about what one scan of the
// tracked tree costs, and shrinks as more is excluded.
function resolveExclusions(root, extra = []) {
  const names = new Set(EXCLUDED_NAMES);
  const literal = new Set(extra.map((value) => String(value).replaceAll("\\", "/")));
  const found = new Set(literal);
  const walk = (absolute, rel) => {
    let entries;
    try { entries = readdirSync(absolute, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      // Never follow symlinked directories: they can leave the tree entirely
      // and can form cycles, and the scanner does not follow them either.
      if (!entry.isDirectory()) continue;
      const childRel = rel ? `${rel}/${entry.name}` : entry.name;
      if (names.has(entry.name) || literal.has(childRel)) { found.add(childRel); continue; }
      walk(join(absolute, entry.name), childRel);
    }
  };
  walk(canonicalRoot(root), "");
  // Bare names stay in the list: they are what excludes a root-level match, and
  // they cost nothing extra.
  return [...new Set([...names, ...found])];
}

function normalizeEvents(root, events, observedMs = Date.now(), transientPaths = []) {
  const normalizedRoot = canonicalRoot(root);
  const transient = new Set(transientPaths.map((path) => normalizePath(normalizedRoot, path)));
  const candidates = events
    .filter((event) => EVENT_TYPES.has(event.type))
    .filter((event) => {
      if (event.type === "delete") return true;
      try { return statSync(event.path).isFile(); }
      catch { return true; }
    })
    .map((event) => ({
      eventKind: event.type === "update" ? "modify" : event.type,
      path: normalizePath(normalizedRoot, event.path),
      renameTo: null,
      observedMs,
    }))
    .filter((event) => event.path && event.path !== ".");
  const deletes = candidates.filter((event) => event.eventKind === "delete");
  const creates = candidates.filter((event) => event.eventKind === "create");
  const paired = new Set();
  for (const deletion of deletes) {
    const match = creates.find((creation, index) => {
      if (paired.has(index) || Math.abs(creation.observedMs - deletion.observedMs) > 50) return false;
      return basename(creation.path) === basename(deletion.path);
    });
    if (!match) continue;
    const index = creates.indexOf(match);
    paired.add(index);
    deletion.eventKind = "rename";
    deletion.renameTo = match.path;
  }
  const pairedCreates = new Set([...paired].map((index) => creates[index]));
  return candidates
    .filter((event) => !pairedCreates.has(event))
    .filter((event) => !transient.has(event.path) && !transient.has(event.renameTo));
}

function options(root, ignore) {
  return { ignore: resolveExclusions(root, ignore) };
}

export async function startWatch(root, onEvents, onGap = () => {}, ignore = [], runtime = {}) {
  const absoluteRoot = canonicalRoot(root);
  const { signal, probeCadenceMs = PROBE_CADENCE_MS, probeDeadlineMs = PROBE_DEADLINE_MS, now = Date.now, subscribe = ParcelWatcher.subscribe, writeProbe = writeFileSync, unlinkProbe = unlinkSync } = runtime;
  let probePath = null;
  const transientPaths = new Set();
  let resolveReady;
  let rejectReady;
  let reportedGap = false;
  const reportGap = (error) => {
    if (reportedGap || signal?.aborted || error?.code === "request_cancelled") return;
    reportedGap = true;
    onGap(error);
  };
  const callback = (error, events = []) => {
    if (error) {
      const failure = signal?.aborted ? cancelled() : error;
      reportGap(failure);
      rejectReady?.(failure);
      return;
    }
    if (probePath && events.some((event) => canonicalAbsolute(event.path) === canonicalAbsolute(probePath))) resolveReady?.();
    try {
      const normalized = normalizeEvents(absoluteRoot, events, Date.now(), [...transientPaths]);
      if (normalized.length) onEvents(normalized);
    } catch (callbackError) { reportGap(callbackError); }
  };
  let subscription = null;
  try {
    subscription = await subscribe(absoluteRoot, callback, options(absoluteRoot, ignore));
    const deadline = now() + probeDeadlineMs;
    let ready = false;
    while (!ready && now() < deadline) {
      if (signal?.aborted) throw cancelled();
      probePath = join(absoluteRoot, `${PROBE_PREFIX}${randomUUID()}`);
      transientPaths.add(probePath);
      const readyProbe = new Promise((resolveReadyPromise, rejectReadyPromise) => { resolveReady = resolveReadyPromise; rejectReady = rejectReadyPromise; });
      writeProbe(probePath, "");
      try {
        await waitForNativeProbe(readyProbe, Math.max(1, Math.min(probeCadenceMs, deadline - now())), signal);
        ready = true;
      } catch (error) {
        if (signal?.aborted || error?.code === "request_cancelled") throw cancelled();
        if (error?.code !== "watch_probe_timeout" || now() >= deadline) throw error;
      } finally {
        try { unlinkProbe(probePath); } catch {}
        probePath = null;
        resolveReady = null;
        rejectReady = null;
      }
    }
    if (!ready) throw Object.assign(new Error("native watcher readiness probe timed out"), { code: "watch_probe_timeout" });
    return {
      async unsubscribe() {
        try { await subscription.unsubscribe(); }
        finally { transientPaths.clear(); }
      },
    };
  } catch (error) {
    reportGap(error);
    if (subscription) await subscription.unsubscribe();
    throw error;
  } finally {
    if (probePath) try { unlinkProbe(probePath); } catch {}
  }
}

export async function writeSnapshot(root, snapshotPath, ignore = []) {
  return ParcelWatcher.writeSnapshot(canonicalRoot(root), resolve(snapshotPath), options(root, ignore));
}

export async function eventsSince(root, snapshotPath, ignore = []) {
  const normalizedRoot = canonicalRoot(root);
  const events = await ParcelWatcher.getEventsSince(normalizedRoot, resolve(snapshotPath), options(normalizedRoot, ignore));
  return normalizeEvents(normalizedRoot, events);
}

export { normalizeEvents };
