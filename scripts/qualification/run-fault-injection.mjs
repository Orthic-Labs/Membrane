#!/usr/bin/env node
import { writeFile } from 'node:fs/promises';
import { runMatrix } from '../../tests/fault-injection/fault-runner.mjs';
const output = process.argv[2];
if (!output) { console.error('usage: run-fault-injection OUTPUT.json'); process.exit(2); }
const receipts = runMatrix({ now: 1_700_000_000_000, deadlineMs: 1500 });
await writeFile(output, `${JSON.stringify({ schemaVersion: 1, kind: 'membrane-fault-suite', receipts }, null, 2)}\n`);
console.log(JSON.stringify({ scenarios: receipts.length, output }));
