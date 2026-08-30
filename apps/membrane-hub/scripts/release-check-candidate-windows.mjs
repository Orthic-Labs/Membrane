import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, lstatSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { validateCycloneDxSbom, validateInTotoSlsaProvenance } from "@rightkit/release/supply-chain-evidence.mjs";

if (process.platform !== "win32") throw new Error("Windows candidate check must run on Windows");
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const artifactRoot = process.env.RIGHT_GIT_ARTIFACT_ROOT;
if (!artifactRoot) throw new Error("RIGHT_GIT_ARTIFACT_ROOT is required");
const candidateManifestPath = join(artifactRoot, "candidate.json");
if (!existsSync(candidateManifestPath)) throw new Error("candidate.json is missing");
const candidate = JSON.parse(readFileSync(candidateManifestPath, "utf8"));
const signingStatus = candidate.signing?.status;
if (candidate.schemaVersion !== 1 || !["membrane-signed-release-candidate", "membrane-unsigned-release-candidate"].includes(candidate.kind) || candidate.product !== "membrane" || candidate.target !== "windows-x86_64") throw new Error("candidate identity is invalid");
if (!/^[0-9a-f]{40}$/.test(candidate.sourceCommit)) throw new Error("candidate source commit is invalid");
if (!/^\d+$/.test(candidate.github?.runId ?? "") || !/^\d+$/.test(candidate.github?.runAttempt ?? "")) throw new Error("candidate GitHub run identity is invalid");
if (!new RegExp(`^membrane-[0-9A-Za-z.+-]+-windows-x86_64-${signingStatus ?? "(?:signed|unsigned)"}\\.zip$`).test(candidate.archive?.name)) throw new Error("candidate archive name is invalid");
if (!candidate.startedAt || Number.isNaN(Date.parse(candidate.startedAt))) throw new Error("candidate start time is invalid");
for (const name of ["membrane-hub.exe", "cortex.exe", "membrane.exe", "membrane-tray.exe", "membrane-daemon.exe"]) {
  if (!candidate.files?.[name]) throw new Error(`candidate executable closure missing: ${name}`);
}
for (const name of [
  "mcp/hooks/membrane-hook-entrypoint.mjs",
  "mcp/hooks/membrane-hook-runtime.mjs",
  "mcp/hooks/membrane-workspace-operations.mjs",
  "mcp/lib/verification-command.mjs",
  "mcp/lib/diagnostics-client.mjs",
  "mcp/host/context-adapter.cjs",
  "mcp/host/continuity.mjs",
  "mcp/host/delivery-ledger-store.cjs",
  "mcp/host/observable-event.cjs",
  "mcp/host/observable-ingress.cjs",
  "mcp/context-renderer-lib.cjs",
]) {
  if (!candidate.files?.[name]) throw new Error(`candidate hook projection closure missing: ${name}`);
}
if (!Object.keys(candidate.files ?? {}).some((name) => name.startsWith("runtime/"))) throw new Error("candidate runtime closure is missing");
const archive = join(artifactRoot, candidate.archive.name);
if (!existsSync(archive)) throw new Error("candidate archive is missing");
const bytes = readFileSync(archive);
if (bytes.length !== candidate.archive.size) throw new Error("candidate archive size mismatch");
if (createHash("sha256").update(bytes).digest("hex") !== candidate.archive.sha256) throw new Error("candidate archive digest mismatch");
if (signingStatus !== "signed" && signingStatus !== "unsigned") throw new Error("candidate signing status is invalid");
const installerRecord = candidate.installer;
if (!installerRecord?.name || !/^[A-Za-z0-9_.+-]+\.exe$/i.test(installerRecord.name)) throw new Error("candidate installer identity is missing");
const installer = join(artifactRoot, installerRecord.name);
if (!existsSync(installer)) throw new Error("candidate installer is missing");
const installerBytes = readFileSync(installer);
if (installerBytes.length !== installerRecord.size || createHash("sha256").update(installerBytes).digest("hex") !== installerRecord.sha256) throw new Error("candidate installer digest mismatch");
const manifestPath = join(artifactRoot, "release-manifest.json");
const sbomPath = join(artifactRoot, "sbom.json");
if (!existsSync(manifestPath) || !existsSync(sbomPath)) throw new Error("candidate qualification evidence is incomplete");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const sbom = JSON.parse(readFileSync(sbomPath, "utf8"));
if (manifest.schema !== "membrane.release-evidence.v1" || manifest.product !== "Membrane Hub" || manifest.artifact?.sha256?.toLowerCase() !== installerRecord.sha256.toLowerCase() || !manifest.artifact?.path) throw new Error("release manifest is not installer-bound");
if (sbom.schema !== "membrane.sbom.v1" || sbom.artifact?.sha256?.toLowerCase() !== installerRecord.sha256.toLowerCase() || !sbom.artifact?.path) throw new Error("SBOM is not installer-bound");
if (manifest.signing?.status !== signingStatus || sbom.signing?.status !== signingStatus || candidate.installer.signing?.status !== signingStatus) throw new Error("candidate signing label is inconsistent");
const evidenceNames = new Set([`sbom-windows-x86_64-${signingStatus}.cdx.json`, `provenance-windows-x86_64-${signingStatus}.intoto.jsonl`]);
if (!Array.isArray(candidate.evidence) || candidate.evidence.length !== evidenceNames.size || candidate.evidence.some((item) => !evidenceNames.delete(item.name)) || evidenceNames.size) throw new Error("candidate evidence closure is invalid");
for (const evidence of candidate.evidence) {
  const path = join(artifactRoot, evidence.name);
  if (!existsSync(path)) throw new Error(`candidate evidence missing: ${evidence.name}`);
  const evidenceBytes = readFileSync(path);
  if (evidenceBytes.length !== evidence.size || createHash("sha256").update(evidenceBytes).digest("hex") !== evidence.sha256) throw new Error(`candidate evidence digest mismatch: ${evidence.name}`);
}
const expectedSubject = { name: candidate.archive.name, sha256: candidate.archive.sha256 };
validateCycloneDxSbom(join(artifactRoot, `sbom-windows-x86_64-${signingStatus}.cdx.json`), { expectedFile: expectedSubject });
const provenance = validateInTotoSlsaProvenance(join(artifactRoot, `provenance-windows-x86_64-${signingStatus}.intoto.jsonl`), { expectedSubject });
if (!provenance.predicate.buildDefinition.resolvedDependencies[0].uri.endsWith(`@${candidate.sourceCommit}`)) throw new Error("candidate provenance source mismatch");
const extracted = mkdtempSync(join(tmpdir(), "membrane-candidate-check-"));
try {
  // Resolve the System32 bsdtar explicitly: PATH often finds GNU tar (Git
  // Bash) first, which reads the colon in an absolute Windows path (C:\...)
  // as a remote host spec and fails with "Cannot connect to C:".
  const tarExe = join(process.env.SystemRoot ?? "C:\Windows", "System32", "tar.exe");
  const unpack = spawnSync(tarExe, ["-xf", archive, "-C", extracted], { windowsHide: true });
  if (unpack.error || unpack.status !== 0) throw new Error("candidate archive extraction failed");
  const walk = (root) => readdirSync(root).flatMap((entry) => {
    const path = join(root, entry);
    if (lstatSync(path).isSymbolicLink()) throw new Error(`candidate contains link: ${path}`);
    return statSync(path).isDirectory() ? walk(path) : [path];
  });
  const actual = Object.fromEntries(walk(extracted).map((path) => [relative(extracted, path).replaceAll("\\", "/"), createHash("sha256").update(readFileSync(path)).digest("hex")]));
  if (JSON.stringify(actual) !== JSON.stringify(candidate.files)) throw new Error("candidate file closure mismatch");
} finally {
  rmSync(extracted, { recursive: true, force: true });
}
const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8", windowsHide: true });
if (head.error || head.status !== 0 || head.stdout.trim() !== candidate.sourceCommit) throw new Error("candidate source commit does not match checkout");
console.log(JSON.stringify({ ok: true, sourceCommit: candidate.sourceCommit, archive: candidate.archive }));
