// MBR-903: maps the four logical release targets (release/contracts/platforms.v1.json,
// already relied on by scripts/release/orchestrate-release.mjs and
// scripts/release/verify-release-evidence.mjs) onto the RightKit platform
// directory that actually produces each one, if any.
//
// This is read-only, declarative data. It does not build anything; it tells
// the orchestrator where on disk to look for what RightKit already sealed,
// and is honest about the two targets RightKit's current apps/membrane-hub
// configuration does not produce yet -- it never fabricates a "verified"
// status for a target nothing built.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const CONTRACTS_ROOT = resolve(HERE, "../../../release/contracts");

export const PLATFORMS = JSON.parse(readFileSync(resolve(CONTRACTS_ROOT, "platforms.v1.json"), "utf8"));
export const PIPELINE = JSON.parse(readFileSync(resolve(CONTRACTS_ROOT, "pipeline.v1.json"), "utf8"));

// RightKit's build-release.mjs seals each build under
// `.right-release/sealed/<releaseId>/<platformDir>/release-manifest.json`,
// where platformDir is "mac" or "windows" -- see
// tools/rightkit/packages/release/release-state.mjs's runBuildStateMachine.
// apps/membrane-hub/right-release.config.mjs's targets.mac currently
// produces one arm64 DMG (macDmg names "_aarch64.dmg" explicitly) and
// targets.win produces one x64 NSIS installer. Windows arm64 is documented
// in docs/release/pipeline.md as "conformance-only until an installed
// receipt promotes it." mac-x64 has no distinct RightKit build target
// today -- a real, confirmed gap in apps/membrane-hub's packaging config,
// which is outside this task's allowed paths (apps/membrane-hub/** belongs
// to the landed MBR-901/MBR-902 lanes). This map records that gap instead
// of hiding it.
export const TARGET_MAP = {
  "mac-arm64": { rightKitPlatform: "mac", platformDir: "mac", buildable: true },
  "mac-x64": {
    rightKitPlatform: null,
    platformDir: null,
    buildable: false,
    reason: "apps/membrane-hub/right-release.config.mjs targets.mac packages only an aarch64 DMG today; no distinct mac-x64 RightKit build exists to read",
  },
  "windows-x64": { rightKitPlatform: "win", platformDir: "windows", buildable: true },
  "windows-arm64": {
    rightKitPlatform: null,
    platformDir: null,
    buildable: false,
    reason: "windows-arm64 is conformance-only per release/contracts/platforms.v1.json until an installed receipt promotes it",
  },
};

export function targetsInOrder() {
  return Object.keys(PLATFORMS.platforms);
}

export function describeTarget(target) {
  const platform = PLATFORMS.platforms[target];
  const mapping = TARGET_MAP[target];
  if (!platform || !mapping) throw new Error(`FAIL CLOSED: unknown release target: ${target}`);
  return { target, ...platform, ...mapping };
}
