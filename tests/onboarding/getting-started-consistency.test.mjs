// MBR-1002 — mechanical anti-drift check for docs/getting-started.md.
//
// The doc is prose; nothing forces it to stay true as the CLIs it documents
// change. This file reads the real source (mcp/install.mjs, mcp/client.mjs,
// mcp/server.mjs, mcp/project-registry.mjs, mcp/installation-binding.mjs,
// package.json, README.md, docs/support-matrix.json) as text and asserts the
// doc's literal commands, env vars, field names, and support-tier claim are
// still consistent with it. No cargo/pnpm build, no network, no service —
// pure fs reads and string/regex checks, so `node --test` alone proves it.
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const read = (relPath) => readFileSync(join(root, relPath), "utf8");

const doc = read("docs/getting-started.md");
const installSrc = read("mcp/install.mjs");
const clientSrc = read("mcp/client.mjs");
const serverSrc = read("mcp/server.mjs");
const projectRegistrySrc = read("mcp/project-registry.mjs");
const installationBindingSrc = read("mcp/installation-binding.mjs");
const pkg = JSON.parse(read("package.json"));
const readme = read("README.md");
const supportMatrix = JSON.parse(read("docs/support-matrix.json"));

test("doc's Node/pnpm claim matches package.json engines and packageManager", () => {
  assert.match(doc, /Node 20\+ & pnpm 11/);
  assert.equal(pkg.engines.node, ">=20");
  assert.match(pkg.packageManager, /^pnpm@11\./);
});

test("doc's enrollment command matches mcp/install.mjs's real init usage", () => {
  assert.match(doc, /node mcp\/install\.mjs init "\$PWD" --repository demo-repo --scope demo-scope/);
  assert.match(installSrc, /membrane init <root> --repository <id> --scope <id>/);
});

test("doc's activation command matches mcp/install.mjs's real install usage and client set", () => {
  assert.match(doc, /node mcp\/install\.mjs install "\$PWD" --client codex/);
  assert.match(doc, /--client claude --claude-scope project/);
  assert.match(installSrc, /membrane install <root> \[--client codex\|claude\]/);
  assert.match(installSrc, /CLIENTS = new Set\(\["codex", "claude"\]\)/);
});

test("doc's enrollment env var is the one project-registry.mjs actually reads", () => {
  assert.match(doc, /MEMBRANE_PROJECT_REGISTRY/);
  assert.match(projectRegistrySrc, /process\.env\.MEMBRANE_PROJECT_REGISTRY/);
});

test("doc's forced-degradation env var is the one client.mjs actually reads, with its real valid range", () => {
  assert.match(doc, /CRYPT_PORT=59991/);
  assert.match(clientSrc, /process\.env\.CRYPT_PORT/);
  assert.match(clientSrc, /n >= 1024 && n <= 65535/);
});

test("doc's forced-degradation expectation matches client.mjs's real transport-failure envelope", () => {
  assert.match(doc, /providerStatus: "unavailable"/);
  assert.match(doc, /degradationReason: "planner_unavailable"/);
  assert.match(doc, /packet: null/);
  assert.match(clientSrc, /providerStatus: "unavailable"/);
  assert.match(clientSrc, /degradationReason: "planner_unavailable"/);
});

test("doc's membrane_context payload shape matches the live tool's required schema", () => {
  assert.match(doc, /"task":"orient me","repository":"\$PWD","caller":\{"root":"\$PWD","repositoryId":"demo-repo","scopeId":"demo-scope"\}/);
  assert.match(serverSrc, /name: "membrane_context"[\s\S]{0,400}?required: \["task", "repository", "caller"\]/);
  assert.match(serverSrc, /required: \["root", "repositoryId", "scopeId"\]/);
});

test("doc's parent-workspace prerequisite matches installation-binding.mjs's real runtime-file walk", () => {
  assert.match(doc, /tools\/lib\/memory\/runtime\.json/);
  assert.match(installationBindingSrc, /RUNTIME_FILE = join\("tools", "lib", "memory", "runtime\.json"\)/);
});

test("doc's README cross-reference anchor exists", () => {
  assert.match(readme, /^## Repository posture$/m);
});

test("doc's support-tier claim matches the generated support-matrix exactly (fails if the matrix changes and the doc doesn't)", () => {
  const match = doc.match(/(\d+)\s+of\s+(\d+)\s+platform\/client\s+pairs\s+qualified/);
  assert.ok(match, "doc must state its support-tier claim as \"N of M platform/client pairs qualified\"");
  const [, claimedQualified, claimedTotal] = match.map(Number);
  const actualTotal = supportMatrix.rows.length;
  const actualQualified = supportMatrix.rows.filter((row) => row.tier === "qualified").length;
  assert.equal(claimedTotal, actualTotal, "doc's total platform/client pair count is stale");
  assert.equal(claimedQualified, actualQualified, "doc claims a qualified count the support matrix does not currently back");
  // MBR-808 contract: this doc must never claim more than the matrix backs.
  assert.ok(claimedQualified <= actualQualified);
});

test("doc's offline fixture commands point at files that exist", () => {
  assert.match(doc, /node examples\/quickstart\/run\.mjs/);
  assert.match(doc, /node examples\/quickstart\/run\.mjs --degraded/);
  // Resolvable without throwing — proves the path is real, not aspirational.
  read("examples/quickstart/run.mjs");
});

test("doc's live-path MCP server dependency claim matches package.json", () => {
  assert.match(doc, /@modelcontextprotocol\/server/);
  assert.ok(Object.prototype.hasOwnProperty.call(pkg.dependencies || {}, "@modelcontextprotocol/server"));
  assert.match(serverSrc, /from "@modelcontextprotocol\/server"/);
});
