#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const fixture = process.env.MEMBRANE_QUICKSTART_FIXTURE || join(fileURLToPath(new URL(".", import.meta.url)), "fixture");
const degraded = process.argv.includes("--degraded");
async function load(name) {
  try { return JSON.parse(await readFile(join(fixture, name), "utf8")); }
  catch (error) { throw new Error(`missing ${name}: ${error.message}`); }
}
const receipt = await load("enrollment-receipt.json");
if (receipt.status !== "enrolled" || !receipt.receipt_id || !receipt.repository_id || !receipt.scope_id) throw new Error("enrollment receipt is invalid; refusing packet");
if (!degraded) {
  const service = await readFile(join(fixture, "service.ready"), "utf8").catch(() => "");
  if (service.trim() !== "ready") throw new Error("service/receipt absent; refusing non-zero delivery");
  process.stdout.write(JSON.stringify({ executionMode: "fixture", evidenceAuthority: "synthetic", packet: { schema: "orthic.context-packet.v1", task: "orient me", blocks: [{ source: "fixture", text: "Membrane is receipt-bound." }] }, receipts: [{ receipt_id: receipt.receipt_id, repository_id: receipt.repository_id, scope_id: receipt.scope_id, status: "fixture-admitted" }], providerStatus: "fixture", degradationReason: "fixture_only" }, null, 2) + "\n");
} else process.stdout.write(JSON.stringify({ executionMode: "fixture", evidenceAuthority: "synthetic", packet: null, receipts: [{ receipt_id: receipt.receipt_id, status: "degraded" }], providerStatus: "degraded", degradationReason: "service_unavailable", omissions: ["service_unavailable"] }, null, 2) + "\n");
