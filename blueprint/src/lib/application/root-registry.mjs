import { existsSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
import { repositoryIdentity } from "../../graph/static-provider.mjs";
import { BlueprintError } from "./errors.mjs";

function canonical(value) {
  const absolute = resolve(String(value));
  return existsSync(absolute) ? realpathSync.native(absolute) : absolute;
}

function rootNotEnrolled(input = {}) {
  const normalizedRoot = input.repoRoot === undefined ? null : canonical(input.repoRoot);
  const nextOperation = normalizedRoot
    ? `blueprint init --root ${JSON.stringify(normalizedRoot)}`
    : "blueprint init --root <repository-root>";
  return new BlueprintError(
    "root_not_enrolled",
    normalizedRoot
      ? `Blueprint root is not enrolled: ${normalizedRoot}`
      : "No enrolled Blueprint repository matches this request.",
    {
      ...(normalizedRoot ? { normalizedRoot } : {}),
      remediation: {
        summary: "Enroll the normalized Blueprint root before querying.",
        nextOperation,
        arguments: normalizedRoot ? { repoRoot: normalizedRoot } : {},
      },
    },
  );
}

export class RootRegistry {
  #byRepoId = new Map();
  #byRoot = new Map();

  constructor(entries = []) {
    for (const entry of entries) this.add(entry);
  }

  add(entry) {
    const root = canonical(entry.root);
    const identity = repositoryIdentity(root);
    const repoId = String(entry.repoId ?? identity.repoId);
    const normalized = Object.freeze({
      repoId,
      root,
      installationId: entry.installationId ?? identity.installationId ?? root,
      worktreeId: entry.worktreeId ?? root,
      enabled: entry.enabled !== false,
    });
    this.#byRepoId.set(repoId, normalized);
    this.#byRoot.set(root, normalized);
    return normalized;
  }

  resolve(input = {}) {
    const byId = input.repoId ? this.#byRepoId.get(String(input.repoId)) : null;
    const byPath = input.repoRoot ? this.#byRoot.get(canonical(input.repoRoot)) : null;
    // An explicit repoRoot that is not enrolled must never be silently ignored,
    // even when a repoId also resolves (D06: resolve only an enrolled repoId or
    // an exact enrolled root).
    if (input.repoRoot !== undefined && !byPath) {
      throw rootNotEnrolled(input);
    }
    // The single-entry fallback applies only when the caller names no explicit
    // selector. An explicit repoRoot/repoId that is not enrolled must never
    // silently select a different repository (D06 root confinement).
    const entry = byId ?? byPath ?? (input.repoId === undefined && input.repoRoot === undefined && this.#byRepoId.size === 1 ? [...this.#byRepoId.values()][0] : null);
    if (!entry || !entry.enabled) {
      throw rootNotEnrolled(input);
    }
    if (byId && byPath && byId.repoId !== byPath.repoId) {
      throw new BlueprintError("root_escape", "Repository ID and root resolve to different enrollments.");
    }
    return entry.root;
  }

  list() {
    return [...this.#byRepoId.values()].map((entry) => ({ ...entry }));
  }
}
