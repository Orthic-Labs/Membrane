export { BlueprintRepositoryWorker, RepositoryActor } from "../../watchman/repo-actor.mjs";
// §7.1 item 5 — watcher-emitted findings bind to the sealed generation through
// the same emission-time binder the resident service uses: one binding
// implementation, one generation model (finalizeGenerationIdentity), never a
// second identity path.
export { buildGenerationBoundBundle } from "../lib/findings/service.mjs";
