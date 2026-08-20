import { readFileSync } from 'node:fs';

const request = JSON.parse(readFileSync(0, 'utf8'));
process.stdout.write(JSON.stringify({
  ok: true,
  providerStatus: 'ready',
  degradationReason: 'none',
  packet: { schema: 'membrane.context-packet.v1', task: request.task, content: 'source-backed data' },
  receipts: [{ id: 'receipt-1' }],
}));
