// Canonical Membrane MCP prompts — legacy (JS) surface.
//
// Every prompt definition lives in `schemas/registry/prompts/*.json` so the native
// (Rust) MCP server and this legacy (JS) MCP server serve the exact same
// payload. This module reads the canonical JSON at runtime, exposes the same
// `listPrompts()` / `getPrompt(name)` shape the Rust module emits, and
// returns Membrane-specific metadata (`operations`, `authorityScope`,
// `authorityEscalation`, `authorityNotes`) so a client can prove the prompt
// is bounded before invoking any tool.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..");
const PROMPT_FILES = ["recap.v1.json", "plan.v1.json", "summarize.v1.json", "checkpoint.v1.json"];

let cached = null;

async function loadAll() {
  if (cached) return cached;
  const entries = await Promise.all(PROMPT_FILES.map(async (file) => {
    const raw = await readFile(join(REPO_ROOT, "schemas", "registry", "prompts", file), "utf8");
    return JSON.parse(raw);
  }));
  cached = entries;
  return cached;
}

function shape(prompt, includeMessages) {
  return {
    name: prompt.name,
    description: prompt.description,
    arguments: prompt.arguments ?? [],
    operations: prompt.operations ?? [],
    authorityScope: prompt.authorityScope ?? null,
    authorityEscalation: prompt.authorityEscalation === true,
    authorityNotes: prompt.authorityNotes ?? null,
    ...(includeMessages ? { messages: prompt.messages ?? [] } : {}),
  };
}

export const PROMPT_NAMES = ["recap", "plan", "summarize", "checkpoint"];

export async function listPrompts() {
  const entries = await loadAll();
  return { prompts: entries.map((prompt) => shape(prompt, false)) };
}

export async function getPrompt(name) {
  const entries = await loadAll();
  const found = entries.find((prompt) => prompt.name === name);
  if (!found) return null;
  return shape(found, true);
}

export async function allPromptDefinitions() {
  return loadAll();
}
