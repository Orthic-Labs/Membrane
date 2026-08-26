import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(fileURLToPath(import.meta.url));
const rust = readFileSync(join(root, "../src-tauri/src/blueprint.rs"), "utf8");
const main = readFileSync(join(root, "../src-tauri/src/main.rs"), "utf8");

test("installed Blueprint lifecycle uses bundled Windows runtime", () => {
  for (const text of ["BLUEPRINT_RUNTIME_ROOT", "lib", "node.exe", "runtime_root"]) {
    assert.match(rust, new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }
  assert.match(rust, /scripts.*blueprint\.mjs/);
  assert.match(rust, /scripts.*blueprint-watch\.mjs/);
  assert.match(rust, /arg\("enroll"\)/);
  assert.match(rust, /arg\("service"\).*arg\("run"\)/s);
});

test("Hub starts or restarts Blueprint only after Membrane reports Running", () => {
  assert.match(rust, /BLUEPRINT_DAEMON_ENDPOINT/);
  assert.match(rust, /USERPROFILE/);
  assert.match(rust, /Sha256::digest/);
  assert.match(rust, /membrane-blueprint-/);
  assert.match(rust, /taskkill/);
  assert.match(rust, /"\/T", "\/F"/);
  assert.match(main, /mod blueprint;/);
  assert.match(
    main,
    /supervisor\.supervise\(\) == supervisor::ServiceStatus::Running\s*\{\s*match blueprint\.start\(\)/s,
  );
  assert.match(main, /blueprint_supervisor\.supervise\(\)/);
  assert.match(
    main,
    /if service_status == supervisor::ServiceStatus::Running\s*\{[\s\S]*?blueprint_supervisor\.start\(\)/,
  );
  assert.match(
    main,
    /if observed_service_status != supervisor::ServiceStatus::Running\s*\{\s*stop_blueprint_service\(&blueprint_supervisor\);[\s\S]*?supervisor\.start\(\)/,
  );
  assert.match(rust, /enum LifecycleState/);
  for (const state of ["not_configured", "stale", "transport_unavailable", "hub_inactive", "resident_owner_active"]) {
    assert.match(rust, new RegExp(`\\"${state}\\"`));
  }
  assert.match(main, /lifecycle_state_for_error/);
});

test("Hub stops and drains Blueprint whenever Membrane is not Running or exits", () => {
  assert.match(
    main,
    /else \{\s*stop_blueprint_service\(&blueprint_supervisor\);\s*telemetry\.event\(\s*"blueprint_installed",\s*blueprint::LifecycleState::HubInactive\.as_str\(\),\s*Some\("membrane_not_running"\),\s*\);\s*\}/s,
  );
  assert.match(main, /stop_blueprint_service/);
  assert.match(
    main,
    /tauri::RunEvent::ExitRequested[\s\S]*?stop_blueprint_service\(&blueprint\)/,
  );
});

test("missing installed Blueprint fails closed", () => {
  assert.doesNotMatch(rust, /ServiceStatus::NotInstalled/);
  assert.match(rust, /blueprint_runtime_root_invalid/);
  assert.match(main, /resource_dir\(\)[\s\S]*join\("runtime"\)[\s\S]*join\("blueprint"\)/);
});
