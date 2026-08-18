// D29: IaC fact profiles — Terraform/HCL resources and K8s manifests.

import { readFileSync } from "node:fs";

export function extractTerraformFacts(text) {
  const facts = [];
  const resourcePattern = /^\s*resource\s+"([^"]+)"\s+"([^"]+)"/gm;
  let match;
  while ((match = resourcePattern.exec(String(text ?? ""))) !== null) {
    const line = String(text).slice(0, match.index).split(/\r?\n/).length;
    facts.push({ kind: "resource", type: match[1], name: match[2], line });
  }
  return facts;
}

export function extractKubernetesFacts(text) {
  const facts = [];
  const kindPattern = /^\s*kind:\s*([A-Za-z]+)/gm;
  let match;
  while ((match = kindPattern.exec(String(text ?? ""))) !== null) {
    const line = String(text).slice(0, match.index).split(/\r?\n/).length;
    facts.push({ kind: "manifest", kindName: match[1], line });
  }
  return facts;
}

export function profileFile(path) {
  const lower = String(path ?? "").toLowerCase();
  if (/\.tf$/.test(lower)) return "terraform";
  if (/\.ya?ml$/.test(lower)) return "kubernetes";
  return null;
}

export function extractProfileFacts(profile, filePath) {
  const text = readFileSync(filePath, "utf8");
  if (profile === "terraform") return extractTerraformFacts(text);
  if (profile === "kubernetes") return extractKubernetesFacts(text);
  return [];
}
