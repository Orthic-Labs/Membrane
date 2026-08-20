import { createHash, createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { homedir, platform } from "node:os";
import { join } from "node:path";

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().filter((key) => key !== "integrity").map((key) => [key, canonical(value[key])]));
  return value;
}
function keyDir() {
  if (platform() === "win32") return join(process.env.LOCALAPPDATA ?? homedir(), "Orthic", "Blueprint", "state-keys");
  return join(process.env.XDG_STATE_HOME ?? join(homedir(), ".local", "state"), "orthic", "blueprint", "state-keys");
}
function keyPath(root, stateKeyDir = keyDir(), namespace = "install") {
  const canonicalRoot = realpathSync(root);
  const digest = createHash("sha256").update(platform() === "win32" ? canonicalRoot.toLowerCase() : canonicalRoot).digest("hex");
  return join(stateKeyDir, `${namespace === "install" ? "" : `${namespace}-`}${digest}.key`);
}
function readKey(root, options, create) {
  const path = keyPath(root, options?.stateKeyDir, options?.namespace);
  if (existsSync(path)) {
    if (lstatSync(path).isSymbolicLink() || !lstatSync(path).isFile()) throw new Error("state_integrity_invalid");
    const key = readFileSync(path); if (key.length !== 32) throw new Error("state_integrity_invalid"); return key;
  }
  if (!create) throw new Error("state_integrity_invalid");
  mkdirSync(options?.stateKeyDir ?? keyDir(), { recursive: true, mode: 0o700 });
  const key = randomBytes(32);
  try { writeFileSync(path, key, { mode: 0o600, flag: "wx" }); } catch { return readKey(root, options, false); }
  return key;
}
function tag(state, key) { return createHmac("sha256", key).update(JSON.stringify(canonical(state))).digest("hex"); }

export function sealInstallState(root, state, options = {}) {
  return sealLocalState(root, state, { ...options, namespace: "install" });
}

export function verifyInstallState(root, state, options = {}) {
  return verifyLocalState(root, state, { ...options, namespace: "install" });
}

export function sealLocalState(root, state, options = {}) {
  const namespace = options.namespace ?? "install", key = readKey(root, { ...options, namespace }, true);
  return { ...state, integrity: { namespace, keyId: createHash("sha256").update(key).digest("hex").slice(0, 16), tag: tag(state, key) } };
}

export function verifyLocalState(root, state, options = {}) {
  const namespace = options.namespace ?? "install";
  if (!state?.integrity || state.integrity.namespace !== namespace || !/^[a-f0-9]{16}$/.test(state.integrity.keyId ?? "") || !/^[a-f0-9]{64}$/.test(state.integrity.tag ?? "")) throw new Error("state_integrity_invalid");
  const key = readKey(root, { ...options, namespace }, false);
  const expected = Buffer.from(tag(state, key), "hex"), actual = Buffer.from(state.integrity.tag, "hex");
  if (createHash("sha256").update(key).digest("hex").slice(0, 16) !== state.integrity.keyId || !timingSafeEqual(expected, actual)) throw new Error("state_integrity_invalid");
  return state;
}

export function removeInstallStateKey(root, options = {}) { rmSync(keyPath(root, options?.stateKeyDir, "install"), { force: true }); }
