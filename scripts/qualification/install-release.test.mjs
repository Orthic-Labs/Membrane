import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("./install-release.ps1", import.meta.url), "utf8");
const lower = source.toLowerCase();

test("Windows installed qualification is package-only & signature-bound", () => {
  for (const term of [
    "Get-AuthenticodeSignature",
    "Assert-SignedFile $HubExecutable",
    "Assert-BoundEvidence",
    "Get-FileHash",
    "EvidencePath",
    "membrane.windows-installed-qualification.v1",
    "PreviousInstaller",
    "uninstall.exe",
  ]) assert.ok(lower.includes(term.toLowerCase()), term);
  assert.doesNotMatch(lower, /pnpm|cargo|tauri\s+build|right-release\s+(build|sign)/);
  assert.match(lower, /publisher does not match installer publisher/);
  assert.doesNotMatch(lower, /expectedthumbprint|signer does not match installer signer/);
  assert.match(source, /Import-Module \(Join-Path \$PSHOME 'Modules\\Microsoft\.PowerShell\.Security\\Microsoft\.PowerShell\.Security\.psd1'\) -Force -ErrorAction Stop/);
});

test("qualification exercises native stdio MCP discovery & every registry tool", () => {
  assert.match(lower, /stdio-mcp/);
  assert.match(lower, /tools\/list/);
  assert.match(lower, /tools\.count -eq 17/);
  assert.match(lower, /membrane\.toolsets\.v1/);
  for (const name of [
    "membrane_context", "membrane_source_read", "membrane_blueprint",
    "membrane_knowledge_propose", "membrane_checkpoint_save", "membrane_checkpoint_load",
    "membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad",
    "membrane_feedback", "membrane_diagnostic_workspace", "membrane_diagnostic_mutation",
    "membrane_diagnostic_snapshot", "membrane_diagnostic_fence",
    "membrane_diagnostic_capabilities", "membrane_diagnostic_baseline",
    "membrane_diagnostic_provider",
  ]) assert.ok(lower.includes(name), name);
});

test("qualification covers tray, popup, renderer, native cutover & forbidden descendants", () => {
  for (const term of [
    "Shell_TrayWnd", "Find-TrayElement", "Assert-TrayAndPopup", "Assert-RendererWindows",
    "Assert-Dashboard", "msedgewebview2", "Assert-NativeHostCutover",
    "native-only steady-state", "^node(?:\\.exe)?$", "exactly one Blueprint service process", "exactly one Blueprint watcher process", "blueprint\\.mjs.*\\bservice\\b.*\\brun\\b",
    "blueprint-watch\\.mjs.*\\bstart\\b", "blueprintGitProcesses", "blueprintGitConsoleHosts", "-not ($blueprint -or $renderer -or $consoleHost -or $git)", "unexpected",
  ]) assert.ok(lower.includes(term.toLowerCase()), term);
  assert.match(lower, /windows notification area is unavailable/);
  for (const field of ["serviceId", "installationId", "cortexStoreId", "releaseGeneration", "protocolVersion", "schemaVersion", "nativeOnly", "subsystems", "capabilities"]) assert.ok(source.includes(field), field);
  assert.match(lower, /executableSha256/i);
  assert.match(lower, /Get-InstalledContentEvidence/i);
});

test("qualification proves current -> transition -> upgrade or repair state continuity & uninstall residue", () => {
  assert.match(lower, /invoke-installer \$installerpath[\s\S]*(?:invoke-installer \$previouspath[\s\S]*invoke-installer \$installerpath|invoke-installer \$installerpath)/);
  assert.match(lower, /start-andverifyprevioushub \$previousversion/);
  assert.match(lower, /previous signed hub did not remain running during downgrade/);
  assert.match(lower, /downgrade\s*=\s*\$rollback/);
  assert.match(lower, /transitioncontract\s*=\s*'signed-version-liveness-durable-state-v1'/);
  assert.match(lower, /transitioncontract\s*=\s*'first-stable-layout-repair-v1'/);
  assert.match(lower, /same-version repair did not create & switch to a unique version root/);
  assert.match(lower, /durablestate.*preserved/);
  assert.match(lower, /upgradecontract\s*=\s*'full-native-upgrade-uninstall-v1'/);
  for (const field of ["installRootRemoved", "processesRemoved", "shortcutsRemoved", "registryRemoved", "durableStatePreserved"]) assert.ok(source.includes(field), field);
  for (const term of [
    "Save-State", "Assert-State", "native-upgrade-continuity", "roots.data",
    "durable data changed during downgrade", "durable data changed during upgrade",
    "Assert-UninstallResidue", "receipt-owned residue", "shortcut targeting install root",
    "registry install entry",
    "uninstall left current junction",
    "uninstall left versioned payloads",
  ]) assert.ok(lower.includes(term.toLowerCase()), term);
  assert.match(lower, /get-artifactversion/);
  assert.match(lower, /current & previous installers are the same version/);
  assert.match(lower, /previous installer version .* is not older than current/);
  assert.match(lower, /expectedgeneration/);
  assert.match(lower, /forbiddengeneration/);
  assert.match(lower, /second hub invocation did not exit/);
  assert.match(lower, /assert-qualificationprocesstreegone/);
  assert.match(lower, /named pipe remained open/);
});

test("qualification binds exact installed renderer, sidecar, & Blueprint process paths", () => {
  assert.match(lower, /count -eq 1/);
  assert.match(lower, /installerpath/);
  assert.match(lower, /sidecar is missing at inventory path/);
  assert.match(lower, /blueprint process executable is not the inventory-bound node/);
  assert.match(lower, /webview2 renderer is not signed/);
  assert.match(lower, /findings\.get/);
  assert.match(lower, /blueprint recall/);
});

test("qualification binds all four native sidecars", () => {
  for (const sidecar of ["membrane-tray", "membrane-daemon", "membrane-command", "cortex-cli"]) {
    assert.match(lower, new RegExp(sidecar.replace('.', '\\.'), 'i'), sidecar);
  }
});

test("qualification proves installed native Adapt selected-transcript lifecycle", () => {
  for (const term of [
    "Invoke-InstalledAdaptQualification",
    "adapt.user-taste-review.v1",
    "review-contract.json",
    "local-user-review",
    "pending_manifest_sha256",
    "selected-transcript.jsonl",
    "adapt mine --host pi --scope workspace",
    "adapt review --input",
    "review-taste --input",
    "adapt adjudicate-taste --manifest",
    "adapt --db",
    "apply --manifest",
    "recall npm --scope workspace",
    "candidate_set_sha256",
    "sourceBindings",
    "nativeOnly = $true",
    "Python, Pi, OpenCode, and Node are absent",
    "python = $false",
    "pi = $false",
    "openCode = $false",
    "node = $false",
    "checkout = $false",
  ]) assert.ok(source.includes(term), term);
  assert.doesNotMatch(lower, /adapt-installed-qualification/);
  assert.match(lower, /qualificationworkspace[\s\S]*tools\\.cache\\memory\\cortex-engine.db/);
  assert.match(lower, /adapt\s*=\s*\$script:adaptevidence/);
  assert.match(lower, /lifecycle[\s\S]*adapt\s*=\s*'pass'/);
  assert.match(lower, /caller-selected/);
  assert.match(lower, /source.*bindings/);
});

test("qualification proves startup workspace migration is native, strict, atomic, & idempotent", () => {
  for (const term of [
    "PreviousMembraneWorkspaceConfig", "MEMBRANE_WORKSPACE_CONFIG", "Seed-WorkspaceV2Config",
    "schemaVersion = 2", "pythonExecutable", "Assert-WorkspaceConfigMigrated",
    "schemaVersion -eq 3", "pythonExecutable", "workspace config migration left temporary files",
    "workspace-config-v2-to-v3-startup-migration-v1", "upgradeIdempotent", "upgradeSha256",
  ]) assert.ok(source.includes(term), term);
  assert.match(lower, /workspace config hash changed during \$phase/);
  assert.match(lower, /workspaceconfiginitialsha256/);
  assert.match(lower, /tools\\lib\\memory/);
  assert.match(lower, /serviceid = 'membrane-local-v1'/);
  assert.match(lower, /runtime\.json/);
});

test("qualification binds Blueprint requests to Hub-enrolled workspace & typed one-shot states", () => {
  assert.match(source, /PreviousMembraneWorkspaceRoot/);
  assert.match(source, /MEMBRANE_WORKSPACE_ROOT\s*=\s*\$script:QualificationWorkspace/);
  assert.ok(source.indexOf("$env:MEMBRANE_WORKSPACE_ROOT = $script:QualificationWorkspace") < source.indexOf("Start-AndVerifyHub 'initial install'"));
  assert.match(lower, /hub owns enrollment/);
  assert.match(lower, /typedmissing/);
  assert.match(lower, /root_not_enrolled/);
  assert.match(lower, /graph_missing/);
  assert.match(lower, /untyped status/);
  assert.match(lower, /availability\s*=\s*if/);
  assert.match(lower, /state\s*=\s*if \(\$state\)/);
});
