#!/usr/bin/env node
// scripts/qualification/benchmark/run-whole-task-benchmark.mjs — MBR-804 CLI.
//
// Wires CLI flags to benchmarks/whole-task/runner.mjs. This is the manual,
// Book-gate-time entrypoint referenced by docs/evaluation/whole-task-benchmark.md;
// it never runs automatically, never downloads a dataset, and never reruns
// the vector bakeoff. Every real-execution dependency (the corpus, the
// commit-reveal holdout salt, the per-platform host receipts, the per-case
// executor) is a required flag or an injectable option with no default that
// fabricates a result — absent inputs degrade explicitly.
//
//     node scripts/qualification/benchmark/run-whole-task-benchmark.mjs \
//       --corpus <path> --salt <salt> --holdout-commitment <sha256> \
//       --release-generation <gen> --release-client <client> --release-service <service> \
//       --host-receipt-macos <path> --host-receipt-windows <path>
//
// This CLI does not supply an `execute` function (it has no client/model to
// drive); wire one programmatically via runWholeTaskBenchmark({ execute }) or
// extend this CLI at the Book gate once a real installed-path executor exists.

import { runWholeTaskBenchmark, DEFAULT_CORPUS_PATH } from '../../../benchmarks/whole-task/runner.mjs';

export function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--corpus') out.corpusPath = argv[(i += 1)];
    else if (arg === '--salt') out.salt = argv[(i += 1)];
    else if (arg === '--holdout-commitment') out.holdoutCommitment = argv[(i += 1)];
    else if (arg === '--holdout-fraction') out.holdoutFraction = Number(argv[(i += 1)]);
    else if (arg === '--release-commit') out.releaseCommit = argv[(i += 1)];
    else if (arg === '--release-generation') out.releaseGeneration = argv[(i += 1)];
    else if (arg === '--release-client') out.releaseClient = argv[(i += 1)];
    else if (arg === '--release-service') out.releaseService = argv[(i += 1)];
    else if (arg === '--model') (out.models ||= []).push(argv[(i += 1)]);
    else if (arg === '--hardware-macos') out.hardwareMacos = argv[(i += 1)];
    else if (arg === '--hardware-windows') out.hardwareWindows = argv[(i += 1)];
    else if (arg === '--host-receipt-macos') out.hostReceiptMacos = argv[(i += 1)];
    else if (arg === '--host-receipt-windows') out.hostReceiptWindows = argv[(i += 1)];
    else if (arg === '--publication-disposition') out.publicationDisposition = argv[(i += 1)];
    else if (arg === '--publication-approved') out.publicationApproved = argv[(i += 1)] === 'true';
  }
  return out;
}

export async function runCli(argv, { execute } = {}) {
  const args = parseArgs(argv);
  return runWholeTaskBenchmark({
    corpusPath: args.corpusPath || DEFAULT_CORPUS_PATH,
    salt: args.salt,
    holdoutCommitment: args.holdoutCommitment,
    holdoutFraction: Number.isFinite(args.holdoutFraction) ? args.holdoutFraction : undefined,
    release: { commit: args.releaseCommit, generation: args.releaseGeneration, client: args.releaseClient, service: args.releaseService },
    models: args.models,
    hardware: { macos: args.hardwareMacos, windows: args.hardwareWindows },
    hostReceiptPaths: { macos: args.hostReceiptMacos, windows: args.hostReceiptWindows },
    publication: { disposition: args.publicationDisposition || 'not-published', approved: args.publicationApproved === true },
    execute,
  });
}

if (import.meta.url === `file://${process.argv[1]}`) {
  runCli(process.argv.slice(2))
    .then((result) => {
      process.stdout.write(`${JSON.stringify({ status: result.status, reason: result.reason }, null, 2)}\n`);
      process.exitCode = result.status === 'ran' ? 0 : 2;
    })
    .catch((error) => {
      process.stderr.write(`${error.message}\n`);
      process.exitCode = 1;
    });
}
