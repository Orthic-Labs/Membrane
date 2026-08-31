import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { materializeHardeningEvidence } from "@rightkit/release/hardening-evidence.mjs";

if (process.platform !== "win32") throw new Error("portable Windows release must run on Windows");
const hub = fileURLToPath(new URL("../", import.meta.url));
const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const candidateRoot = process.env.MEMBRANE_CANDIDATE_ROOT;
if (!candidateRoot) throw new Error("MEMBRANE_CANDIDATE_ROOT must identify downloaded CI candidate artifact");
const manifestPath = join(candidateRoot, "candidate.json");
if (!existsSync(manifestPath)) throw new Error("candidate.json is missing");
const candidate = JSON.parse(readFileSync(manifestPath, "utf8"));
const staged = join(hub, "dist", "portable", "staged-candidate");
const archive = join(candidateRoot, candidate.archive.name);

function run(command, args, cwd = hub, env = process.env) {
  const executable = command === "pnpm" ? "pnpm.cmd" : command;
  const result = spawnSync(executable, args, { cwd, env, stdio: "inherit", shell: executable.endsWith(".cmd"), windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} exited ${result.status}`);
}

run("pnpm", ["run", "release:candidate:check:win"], hub, { ...process.env, RIGHT_GIT_ARTIFACT_ROOT: candidateRoot });
const status = spawnSync("git", ["status", "--porcelain"], { cwd: repoRoot, encoding: "utf8", windowsHide: true });
if (status.error || status.status !== 0 || status.stdout.trim()) throw new Error("release finalization requires clean source");
rmSync(staged, { recursive: true, force: true });
mkdirSync(staged, { recursive: true });
run("tar.exe", ["-xf", archive, "-C", staged]);
const executables = ["membrane-hub.exe", "cortex.exe", "membrane.exe", "membrane-tray.exe", "membrane-daemon.exe"].map((name) => join(staged, name));
run("pnpm", ["exec", "right-release", "sign-windows", ...executables]);
run("pnpm", ["exec", "right-release", "sign-windows", "--verify-only", ...executables]);

const hardeningEvidence = join(hub, "dist", "portable", "windows-hardening-evidence.json");
materializeHardeningEvidence({
  root: repoRoot,
  outputPath: hardeningEvidence,
  allowances: ["cortex.exe", "membrane.exe"].map((name) => ({
    rule: "system-prompt-marker",
    exact: "system_prompt",
    file: join(staged, name),
    sourceEvidence: "engine/crates/membrane-adapt/src/remediation.rs:76",
    rationale: "Public remediation scope enum serialization token; no prompt content is embedded.",
  })),
});
run(join(hub, "node_modules", ".bin", "right-release.cmd"), ["hardening", "--allow-evidence", hardeningEvidence, ...executables], repoRoot);
run("node", ["scripts/package-portable-windows.mjs", "--input-root", staged, "--hub-exe", join(staged, "membrane-hub.exe"), "--started-at", candidate.startedAt]);
run("node", ["scripts/finalize-portable-release.mjs"]);
