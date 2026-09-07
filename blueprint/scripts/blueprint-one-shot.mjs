#!/usr/bin/env node
// One bounded, authorized request from Membrane's application transport.
// This process never starts a watcher or registers a resident root.
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { createBuildSingleflight, runLocalBuild } from "../src/service/build-singleflight.mjs";
import { encodeResponse, validateProtocolVersion, validateDeadlineMs } from "../src/service/protocol.mjs";

const methods = new Set(["status", "search", "resolve", "recall", "expand", "impact", "path", "architecture", "documentTruth", "refresh", "build", "snapshot_get", "snapshot_list", "changes"]);
let request;
let timer;
try {
  let bytes = 0;
  const chunks = [];
  for await (const chunk of process.stdin) {
    bytes += chunk.length;
    if (bytes > 65536) throw Object.assign(new Error("request exceeds 64 KiB"), { code: "request_too_large" });
    chunks.push(chunk);
  }
  request = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  validateProtocolVersion(request.protocolVersion);
  validateDeadlineMs(request.deadlineMs, request.method);
  if (!methods.has(request.method) || typeof request.input?.repoRoot !== "string") {
    throw Object.assign(new Error("explicit root and supported operation required"), { code: "invalid_request" });
  }
  const controller = new AbortController();
  timer = setTimeout(() => controller.abort(), request.deadlineMs);
  const service = createBlueprintApplicationService({
    rootRegistry: new RootRegistry([{ root: request.input.repoRoot, repoId: request.repoId }]),
    freshnessOwnership: "one_shot",
    buildSingleflight: createBuildSingleflight({ runner: async (input) => {
      const result = await runLocalBuild(input);
      if (result.exitCode !== 0) process.stderr.write(JSON.stringify({
        event: "blueprint_one_shot_build_failed", exitCode: result.exitCode,
        stderr: result.stderr.slice(-8192),
      }) + "\n");
      return result;
    } }),
  });
  if (request.method === "build") {
    if (request.generation) await service.refresh({ ...request.input, generation: request.generation }, { signal: controller.signal });
    const built = await createBuildSingleflight().build({ root: request.input.repoRoot,
      options: { noReadmeLink: true } }, { signal: controller.signal });
    if (built.exitCode !== 0) throw Object.assign(new Error(built.stderr.slice(-8192)), { code: "graph_build_failed" });
  }
  const result = await service[request.method === "build" ? "refresh" : request.method]({
    ...request.input, generation: request.generation, timeoutMs: request.deadlineMs,
  }, { signal: controller.signal });
  process.stdout.write(encodeResponse({ requestId: request.requestId, ok: true,
    generation: result?.generationId ?? null, result, error: null }));
} catch (error) {
  process.stdout.write(encodeResponse({ requestId: request?.requestId ?? null, ok: false,
    generation: null, result: null, error: { code: error.code ?? "internal_error", message: error.message } }));
} finally {
  clearTimeout(timer);
}
