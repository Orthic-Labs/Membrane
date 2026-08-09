// D29: schema/config/IaC fact profiles — deterministic domain facts only;
// positive, negative, and ambiguous fixtures per profile.

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { profileForPath, extractSqlFacts, extractDockerfileFacts } from "../providers/schemas/sql.mjs";
import { extractTerraformFacts, extractKubernetesFacts, profileFile, extractProfileFacts } from "../providers/iac/terraform.mjs";

test("profileForPath routes by extension and content hints", () => {
  assert.equal(profileForPath("infra/main.tf"), "terraform");
  assert.equal(profileForPath("Dockerfile"), "dockerfile");
  assert.equal(profileForPath("k8s/deployment.yaml"), "kubernetes");
  assert.equal(profileForPath(".github/workflows/ci.yml"), "github-actions");
  assert.equal(profileForPath("api/openapi.json"), "openapi");
  assert.equal(profileForPath("schema.graphql"), "graphql");
  assert.equal(profileForPath("schema.prisma"), "prisma");
  assert.equal(profileForPath("db/migrations/001.sql"), "migration");
  assert.equal(profileForPath("db/schema.sql"), "sql");
  assert.equal(profileForPath("notes.txt"), null);
});

test("SQL facts extract tables, indexes, and migrations (positive)", () => {
  const sql = [
    "-- Migration: add-orders",
    "CREATE TABLE orders (id INTEGER PRIMARY KEY, amount REAL);",
    "CREATE INDEX idx_orders_amount ON orders(amount);",
  ].join("\n");
  const facts = extractSqlFacts(sql);
  assert.ok(facts.some((f) => f.kind === "migration" && f.name === "add-orders"));
  assert.ok(facts.some((f) => f.kind === "table" && f.name === "orders"));
  assert.ok(facts.some((f) => f.kind === "index" && f.name === "idx_orders_amount" && f.table === "orders"));
});

test("SQL facts emit nothing for non-DDL (negative)", () => {
  const facts = extractSqlFacts("SELECT * FROM orders;");
  assert.equal(facts.filter((f) => f.kind === "table").length, 0);
});

test("Dockerfile facts extract FROM/EXPOSE/ENTRYPOINT", () => {
  const facts = extractDockerfileFacts("FROM node:22\nEXPOSE 3000\nENTRYPOINT [\"node\", \"app.js\"]\n");
  assert.ok(facts.some((f) => f.kind === "base_image" && f.name === "node:22"));
  assert.ok(facts.some((f) => f.kind === "port" && f.name === "3000"));
  assert.ok(facts.some((f) => f.kind === "entrypoint"));
});

test("Terraform facts extract resources only from real declarations (ambiguous-safe)", () => {
  const tf = [
    "resource \"aws_db_instance\" \"orders\" {",
    "  # a comment mentioning resource \"fake\" \"thing\"",
    "}",
  ].join("\n");
  const facts = extractTerraformFacts(tf);
  assert.equal(facts.length, 1);
  assert.equal(facts[0].type, "aws_db_instance");
  assert.equal(facts[0].name, "orders");
});

test("Kubernetes facts extract kinds from manifests", () => {
  const yaml = "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n---\nkind: Deployment\n";
  const facts = extractKubernetesFacts(yaml);
  assert.deepEqual(facts.map((f) => f.kindName), ["Service", "Deployment"]);
});

test("profileFile and extractProfileFacts work from disk", () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-iac-"));
  try {
    writeFileSync(join(root, "main.tf"), "resource \"aws_s3_bucket\" \"data\" {}\n");
    assert.equal(profileFile("main.tf"), "terraform");
    const facts = extractProfileFacts("terraform", join(root, "main.tf"));
    assert.equal(facts.length, 1);
    assert.equal(facts[0].name, "data");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
