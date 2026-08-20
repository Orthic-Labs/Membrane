// D11: support bundles contain only allowlisted records and never leak
// secrets, source content, or absolute home paths.

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildSupportBundle, SUPPORT_BUNDLE_ALLOWLIST } from "../src/lib/operations/support-bundle.mjs";
import { redactForEgress } from "../src/lib/redaction.mjs";

const KNOWN_SECRETS = [
  "ghp_1234567890abcdefghijklmnopqrstuvwxyz",
  "AKIA1234567890ABCDEF",
  "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----",
  "Bearer abcdefghijklmnopqrstuvwxyz.1234567890",
];

test("support bundle contains only allowlisted records", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-bundle-"));
  try {
    const { path, files } = buildSupportBundle({ root, outDir: ".agent", destination: join(root, ".agent", "bundle") });
    for (const file of SUPPORT_BUNDLE_ALLOWLIST) {
      assert.ok(existsSync(join(path, file)), `missing allowlisted record ${file}`);
    }
    assert.ok(Array.isArray(files));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("no known secret appears in the bundle", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-bundle-secret-"));
  try {
    const { path } = buildSupportBundle({
      root,
      outDir: ".agent",
      destination: join(root, ".agent", "bundle"),
      records: {
        watchmanLog: KNOWN_SECRETS.join("\n"),
        serviceLog: `token=${KNOWN_SECRETS[0]}`,
        installation: { root, home: homedir(), apiKey: KNOWN_SECRETS[2] },
      },
    });
    const allText = [];
    const walk = (dir) => {
      for (const name of readdirSync(dir)) {
        const p = join(dir, name);
        if (statSync(p).isDirectory()) walk(p);
        else if (p.endsWith(".json") || p.endsWith(".log") || p.endsWith(".txt")) allText.push(readFileSync(p, "utf8"));
      }
    };
    walk(path);
    const joined = allText.join("\n");
    for (const secret of KNOWN_SECRETS) {
      assert.ok(!joined.includes(secret.trim()), `secret leaked: ${secret.slice(0, 20)}...`);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("absolute home paths are rewritten", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-bundle-home-"));
  try {
    const { path } = buildSupportBundle({
      root,
      outDir: ".agent",
      destination: join(root, ".agent", "bundle"),
      records: { installation: { root, home: homedir() } },
    });
    const installation = JSON.parse(readFileSync(join(path, "installation.json"), "utf8"));
    assert.ok(!JSON.stringify(installation).includes(homedir()), "absolute home path leaked");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("redactForEgress masks secret keys and secret-shaped values", () => {
  const out = redactForEgress({ token: "ghp_abc", apiKey: "x", nested: { password: "pw", ok: "fine" }, plain: "Bearer abc123.def456" });
  assert.equal(out.token, "[REDACTED]");
  assert.equal(out.apiKey, "[REDACTED]");
  assert.equal(out.nested.password, "[REDACTED]");
  assert.equal(out.nested.ok, "fine");
  assert.match(out.plain, /\[REDACTED\]/);
});

test("support bundle logs redact the full secret class, not a stale subset", () => {
  // These token classes are covered by lib/redaction.mjs's SECRET_VALUE but
  // were historically absent from the bundle's own log redaction: Anthropic
  // `sk-ant-` keys, Slack `xox*` tokens, bare JWTs, and URL-password
  // connection strings. None may survive into a bundle log file.
  const root = mkdtempSync(join(tmpdir(), "blueprint-bundle-drift-"));
  try {
    const slackToken = ["xoxb", "1234567890", "abcdefghijklmnop"].join("-");
    const driftSecrets = [
      "sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890",
      slackToken,
      "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
      "postgres://admin:sup3rsecret@db.example.com:5432/app",
    ];
    const { path } = buildSupportBundle({
      root,
      outDir: ".agent",
      destination: join(root, ".agent", "bundle"),
      records: {
        watchmanLog: driftSecrets.join("\n"),
        serviceLog: `db=${driftSecrets[3]} anthropic=${driftSecrets[0]}`,
      },
    });
    const logText = [
      readFileSync(join(path, "logs", "watchman-tail.log"), "utf8"),
      readFileSync(join(path, "logs", "service-tail.log"), "utf8"),
    ].join("\n");
    for (const secret of driftSecrets) {
      assert.ok(!logText.includes(secret), `secret leaked into bundle log: ${secret.slice(0, 20)}...`);
    }
    assert.ok(!logText.includes("sup3rsecret"), "URL password leaked into bundle log");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
