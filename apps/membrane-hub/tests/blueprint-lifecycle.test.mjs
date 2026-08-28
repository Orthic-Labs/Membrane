import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(fileURLToPath(import.meta.url));
const main = readFileSync(join(root, "../src-tauri/src/main.rs"), "utf8");
const connection = readFileSync(join(root, "../src-tauri/src/dashboard_connection.rs"), "utf8");
const cargo = readFileSync(join(root, "../src-tauri/Cargo.toml"), "utf8");
const production = main.split("#[cfg(test)]")[0];

test("dashboard does not own resident Blueprint or daemon lifecycle", () => {
  assert.doesNotMatch(production, /mod blueprint;/);
  assert.doesNotMatch(production, /blueprint(?:_supervisor)?\.(?:start|stop|supervise)\(/);
  assert.doesNotMatch(production, /membrane_runtime_supervisor|run_hub_runtime|std::thread::spawn/);
  assert.doesNotMatch(cargo, /membrane-runtime/);
  assert.match(production, /DashboardConnectionState::from_stdin\(\)/);
});

test("dashboard proxies Blueprint-backed state through authenticated loopback", () => {
  assert.match(connection, /read_bootstrap_from_stdin/);
  assert.match(connection, /Authorization: Bearer/);
  assert.match(connection, /parse_loopback_endpoint/);
  assert.match(production, /connection\.get\("\/hub\/snapshot"/);
  assert.match(production, /connection\.get\("\/health"/);
  assert.doesNotMatch(`${production}\n${connection}`, /api-token|WORKSPACE_ROOT|MEMBRANE_PORT/);
});

test("dashboard close never tears down resident owners", () => {
  assert.doesNotMatch(production, /CloseRequested/);
  assert.doesNotMatch(production, /stop_membrane_service|stop_blueprint_service/);
  assert.match(production, /fn quit_app/);
  assert.match(production, /app\.exit\(0\)/);
});

test("startup registration remains owned by resident tray", () => {
  assert.match(production, /fn set_startup/);
  assert.match(production, /fn startup_setting/);
  assert.match(production, /startup_owned_by_tray/);
  assert.doesNotMatch(production, /LaunchAgents|set_platform_startup|current_app_bundle/);
});
