// Child-process helper for store-lease.test.mjs. Real OS-level file locking
// can only be proven across real OS processes — a single Node process can
// never demonstrate "the OS releases the lock when the holder crashes"
// because nothing in-process actually dies. This script is spawned as its
// own process so tests can kill -9 it and observe the lock's fate.
//
// Modes (argv[3]):
//   hang  — acquire once, print { ready:true, lease }, then block forever
//           until the parent test kills this process. Used for the
//           crashed-owner recovery test.
//   race  — attempt exactly one acquire, print the outcome
//           ({ ok:true, lease } or { ok:false, code }), and on success hold
//           the lease for holdMs (argv[4], default 300) before releasing and
//           exiting cleanly. Used for the concurrent-acquisition test: many
//           copies of this script are spawned at once against the same
//           dbPath, and at most one may ever report ok:true.

import { acquireStoreLease } from "../../src/graph/store-lease.mjs";

const [, , dbPath, mode, holdMsArg] = process.argv;
const holdMs = Number(holdMsArg ?? 300);

function print(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

if (mode === "hang") {
  const handle = acquireStoreLease(dbPath, { ownerKind: "one_shot", ownerInstanceId: `hang-${process.pid}` });
  print({ ready: true, lease: handle.lease });
  setInterval(() => {}, 1000);
} else if (mode === "race") {
  try {
    const handle = acquireStoreLease(dbPath, { ownerKind: "one_shot", ownerInstanceId: `race-${process.pid}` });
    print({ ok: true, lease: handle.lease });
    await new Promise((r) => setTimeout(r, holdMs));
    handle.release();
    process.exit(0);
  } catch (error) {
    print({ ok: false, code: error?.code ?? null, message: error?.message ?? null });
    process.exit(0);
  }
} else {
  process.stderr.write(`unknown mode: ${mode}\n`);
  process.exit(1);
}
