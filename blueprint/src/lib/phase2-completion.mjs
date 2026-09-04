// Automatic completion for deterministic document-domain work.
//
// A document delta updates the canonical graph plus claims/stale/queue artifacts
// synchronously in delta-store.mjs. Phase 2 is a separate judgment/projection
// layer: existing verdicts/understanding may be reusable for the new generation,
// or the new source may require fresh verification/synthesis that the watcher
// must never invent. This service closes that lifecycle without conflating the
// two states:
//
//   doc pending -> deterministic doc artifacts are current -> clear `doc`
//               -> reseal Phase-2 artifacts only when the incremental plan says
//                  every input is reusable
//               -> otherwise persist an explicit pending Phase-2 plan
//
// Clearing is fenced by generation identity. An older completion attempt can
// never clear a `doc` mark belonging to a newer applied source generation.

import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";

import { clearDomainPending, readPendingDomains } from "../graph/delta-store.mjs";
import { readGeneration } from "../graph/static-provider.mjs";
import { getGenerationEnvelope } from "../graph/store-sqlite.mjs";
import { buildIncrementalPhase2Plan, sealPhase2Artifacts } from "./incremental-phase2.mjs";

function readJson(path, fallback = null) {
  if (!existsSync(path)) return fallback;
  return JSON.parse(readFileSync(path, "utf8"));
}

function writeJsonAtomic(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.tmp-${process.pid}-${Date.now()}`;
  try {
    writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, "utf8");
    renameSync(temporary, path);
  } finally {
    rmSync(temporary, { force: true });
  }
}

function setState(db, key, value) {
  db.prepare(
    "INSERT INTO watch_state(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
  ).run(key, String(value));
}

function currentGenerationId(db) {
  return getGenerationEnvelope(db)?.manifest?.generationId ?? null;
}

function phase2Inputs(root, outDir) {
  const base = resolve(root, outDir);
  return {
    graph: readGeneration(root, outDir),
    queue: readJson(join(base, "queue.json"), null),
    verdictEnvelope: readJson(join(base, "verdicts.json"), null),
    understanding: readJson(join(base, "understanding.json"), null),
  };
}

function writeCompletionState(db, { state, generationId, plan }) {
  setState(db, "phase2_completion_state", state);
  if (generationId) setState(db, "phase2_completion_generation", generationId);
  if (plan) {
    setState(db, "phase2_pending_verdicts", plan.verdicts?.verify?.length ?? 0);
    setState(db, "phase2_pending_dimensions", plan.dimensions?.synthesize?.length ?? 0);
  }
}

/**
 * Consume the automatically-produced `doc` pending mark.
 *
 * `doc` means the deterministic document extraction/artifact lane has not yet
 * been acknowledged by its automatic consumer. Phase-2 judgment is recorded
 * independently: when prior verdicts/understanding are reusable they are
 * resealed to the current generation; otherwise `phase2-plan.json` plus
 * watch_state make the remaining judgment work explicit without pinning the
 * source graph stale forever.
 *
 * The caller owns the writable store lease. This function never opens another
 * writable handle or shells out to the CLI.
 */
export function completePendingDocDomain(db, root, {
  outDir = ".agent",
  beforeFinalize = null,
} = {}) {
  if (!readPendingDomains(db).includes("doc")) {
    return Object.freeze({ state: "noop", generationId: currentGenerationId(db), phase2Complete: null });
  }

  const inputs = phase2Inputs(root, outDir);
  const generationId = inputs.graph?.manifest?.generationId ?? null;
  if (!generationId || currentGenerationId(db) !== generationId) {
    return Object.freeze({ state: "superseded", generationId, phase2Complete: null });
  }

  const plan = buildIncrementalPhase2Plan(inputs);
  const base = resolve(root, outDir);

  // The plan is itself the durable receipt for judgment work that cannot be
  // completed deterministically by the watcher.
  if (currentGenerationId(db) !== generationId) {
    return Object.freeze({ state: "superseded", generationId, phase2Complete: plan.complete });
  }
  writeJsonAtomic(join(base, "phase2-plan.json"), plan);

  if (plan.complete) {
    const sealed = sealPhase2Artifacts(inputs);
    if (currentGenerationId(db) !== generationId) {
      return Object.freeze({ state: "superseded", generationId, phase2Complete: true });
    }
    writeJsonAtomic(join(base, "verdicts.json"), sealed.verdicts);
    writeJsonAtomic(join(base, "understanding.json"), sealed.understanding);
  }

  // Test seam and future async-provider seam: even if the source advances after
  // planning/sealing but before publication, the older attempt must fail closed.
  beforeFinalize?.({ db, generationId, plan });

  db.exec("BEGIN IMMEDIATE");
  try {
    if (currentGenerationId(db) !== generationId) {
      db.exec("ROLLBACK");
      return Object.freeze({ state: "superseded", generationId, phase2Complete: plan.complete });
    }

    // Deterministic document extraction is complete regardless of whether the
    // optional judgment/projection layer can be reused. Do not fabricate fresh
    // verdicts merely to make the watcher green.
    clearDomainPending(db, "doc");
    if (plan.complete) clearDomainPending(db, "semantic");
    writeCompletionState(db, {
      state: plan.complete ? "reused" : "pending_judgment",
      generationId,
      plan,
    });
    db.exec("COMMIT");
  } catch (error) {
    try { db.exec("ROLLBACK"); } catch {}
    throw error;
  }

  return Object.freeze({
    state: plan.complete ? "complete" : "doc_current_phase2_pending",
    generationId,
    phase2Complete: plan.complete,
    verify: plan.verdicts?.verify?.length ?? 0,
    synthesize: plan.dimensions?.synthesize?.length ?? 0,
  });
}
