import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";
import { PRESENTATION_ASSETS } from "../scripts/presentation-assets.mjs";

const root = new URL("../", import.meta.url);
const text = (path) => readFileSync(new URL(path, root), "utf8");
const sources = PRESENTATION_ASSETS.map((path) => ({ path, source: text(path) }));

test("bounded presentation has restrictive CSP & one local capability", () => {
  const config = JSON.parse(text("src-tauri/tauri.conf.json"));
  const capability = JSON.parse(text("src-tauri/capabilities/default.json"));
  assert.equal(config.app.security.freezePrototype, true);
  assert.deepEqual(config.app.security.capabilities, ["hub-local-ui"]);
  for (const directive of ["default-src 'self'", "base-uri 'none'", "object-src 'none'", "frame-ancestors 'none'", "script-src 'self'"]) assert.match(config.app.security.csp, new RegExp(directive.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  assert.doesNotMatch(config.app.security.csp, /unsafe-(?:inline|eval)/);
  assert.equal(capability.identifier, "hub-local-ui");
  assert.deepEqual(capability.windows, ["dashboard"]);
  assert.deepEqual(capability.permissions, ["core:default"]);
  const cargo = text("src-tauri/Cargo.toml");
  assert.doesNotMatch(cargo, /tauri-plugin-(?:shell|fs|http|sql|process|upload)/);
});

test("application assets contain presentation only", () => {
  assert.deepEqual(PRESENTATION_ASSETS, [
    "index.html", "src/overview.css", "src/overview.mjs", "src/shell.mjs",
    "assets/fonts/Tanker-400.woff2", "assets/fonts/SplineSansMono-400.woff2",
    "assets/fonts/SplineSansMono-500.woff2",
  ]);
  const builder = text("scripts/build-frontend.mjs");
  assert.match(builder, /for \(const name of PRESENTATION_ASSETS\)/);
  assert.doesNotMatch(builder, /\["index\.html", "popover\.html", "src"\]/);
  const joined = sources.map(({ path, source }) => `/* ${path} */\n${source}`).join("\n");
  assert.doesNotMatch(joined, /(?:from\s*|import\s*\()\s*["']node:/);
  assert.doesNotMatch(joined, /\b(?:require|process|Buffer|eval|Function|Worker|WebSocket|XMLHttpRequest|indexedDB|localStorage)\b/);
  assert.doesNotMatch(joined, /\bfetch\s*\(/);
  assert.doesNotMatch(joined, /(?:node_modules|\.venv|[A-Za-z]:[\\/]|\.\.[\\/].*checkout)/);
  const calls = [...joined.matchAll(/\binvoke\(\s*["']([^"']+)["']/g)].map((match) => match[1]);
  assert.deepEqual([...new Set(calls)].sort(), [
    "diagnostics_report", "snapshot",
  ]);
});

test("dashboard remains a normal visible application surface", () => {
  assert.doesNotMatch(text("src-tauri/Info.plist"), /LSUIElement/);
});

test("bounded source assets have stable content hashes", () => {
  const rows = sources.map(({ path, source }) => ({ path, sha256: createHash("sha256").update(source).digest("hex") }));
  assert.ok(rows.length >= 6);
  assert.equal(new Set(rows.map(({ path }) => path)).size, rows.length);
  assert.ok(rows.every(({ sha256 }) => /^[a-f0-9]{64}$/.test(sha256)));
});

test("HeardRight shell geometry stays intact around Membrane content", () => {
  const index = text("index.html");
  const css = text("src/overview.css");
  assert.match(index, /class="title-slot"[\s\S]*class="title-meta"/);
  assert.match(index, /class="rail"[\s\S]*class="body"/);
  assert.match(css, /grid-template-columns:208px minmax\(0,1fr\);grid-template-rows:40px minmax\(0,1fr\)/);
  assert.match(css, /grid-template-areas:"titlebar titlebar" "sidebar main"/);
  assert.match(css, /border-radius:8px 0 0 8px/);
  assert.match(css, /mask:radial-gradient\(11px at 0% 0%/);
  assert.match(css, /@media\(max-width:720px\)[\s\S]*grid-template-areas:"titlebar" "main"/);
});
