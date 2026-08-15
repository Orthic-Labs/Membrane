#!/usr/bin/env node
// Orthic O5 native lifecycle fixture child — owned by OR-NATIVE-HARNESS.
//
// This is the *only* `<fixture-child>` entry point the native qualification
// matrix references. N-MAC / N-WIN stage it as a supervised product: they
// install a fixture product manifest (tests/native/fixtures/
// fixture-product-manifest.template.json) whose `serviceStart` is this script
// (made executable on Mac; a staged launcher on Windows), stamp its
// `artifactDigest` from the script bytes, then launch the Hub. The Hub's
// supervisor checks the digest, spawns this script, and writes the `hello`
// frame into its inherited stdin; this child then writes `register`/`ack`
// frames to stdout.
//
// Behaviour is selected by `--scenario`:
//   normal             -> valid ready register, acks lifecycle commands
//   degraded           -> register state=degraded (typed degradation)
//   incompatible-range -> register state=incompatible (Hub renders typed incompatible)
//   stale-fence        -> register with fence-1 (rejected: stale_fence)
//   old-instance       -> ready register, then a second stale-fence register
//                         (old-instance adoption rejected)
//   rogue-endpoint     -> ready register with a non-loopback endpoint (rejected)
//   update-handoff     -> ready, then drain on update_handoff and exit clean
//
// `wrong-digest` is intentionally NOT a scenario: the artifact digest is a
// manifest field checked by the Hub *before* launch (supervisor.spawn), so a
// wrong-digest fixture never spawns and never speaks the lifecycle channel.
// The matrix's wrong-digest case stages a fixture manifest whose
// `artifactDigest` does not match the staged bytes and asserts the Hub refuses
// launch with `artifact_digest_mismatch`.
//
// Headless carve-out: when this script is run directly with no Hub (no `hello`
// on stdin within the idle window), it emits ZERO bytes and exits 0 — proving a
// product child never self-registers without a Hub-provided hello and never
// attaches to a lease. It never claims a receipt and never runs a native
// install; it is driven by the Hub supervisor the matrix launches.
import { createInterface } from "node:readline";

const SCENARIOS = new Set([
  "normal", "degraded", "incompatible-range", "stale-fence",
  "old-instance", "rogue-endpoint", "update-handoff",
]);
const IDLE_NO_HELLO_MS = 2000;

function parseArgs(argv) {
  const i = argv.indexOf("--scenario");
  const scenario = i === -1 ? undefined : argv[i + 1];
  if (!SCENARIOS.has(scenario)) {
    process.stderr.write(`unknown or missing --scenario: ${scenario}\n`);
    process.exit(2);
  }
  return { scenario };
}

function send(frame) { process.stdout.write(JSON.stringify(frame) + "\n"); }

function registerFor(scenario, hello) {
  const f = hello.fence;
  const ep = { host: "127.0.0.1", port: 9 };
  switch (scenario) {
    case "stale-fence":
      return { kind: "register", state: "ready", fence: Math.max(0, f - 1), endpoint: ep, capability: "cap" };
    case "incompatible-range":
      return { kind: "register", state: "incompatible", fence: f };
    case "degraded":
      return { kind: "register", state: "degraded", fence: f, endpoint: ep, capability: "cap-degraded" };
    case "rogue-endpoint":
      return { kind: "register", state: "ready", fence: f, endpoint: { host: "10.0.0.1", port: 9 }, capability: "cap" };
    default:
      return { kind: "register", state: "ready", fence: f, endpoint: ep, capability: "cap" };
  }
}

function run(scenario) {
  let hello = null;
  let sawHello = false;
  const rl = createInterface({ input: process.stdin, terminal: false });

  // Idle guard: with no Hub, there is no hello. A product child must never
  // self-register. Exit 0 after the idle window having written zero bytes.
  const idle = setTimeout(() => {
    if (!sawHello) process.exit(0);
  }, IDLE_NO_HELLO_MS);

  rl.on("line", (line) => {
    let frame;
    try { frame = JSON.parse(line); } catch { process.exit(1); }
    if (frame.kind === "hello") {
      sawHello = true;
      clearTimeout(idle);
      hello = frame;
      send(registerFor(scenario, frame));
      if (scenario === "old-instance") {
        // A second register from a prior instance trying to re-adopt the lease.
        send({ kind: "register", state: "ready", fence: Math.max(0, frame.fence - 1), endpoint: { host: "127.0.0.1", port: 9 }, capability: "cap-old" });
      }
    } else if (frame.kind === "command") {
      send({ kind: "ack", command: frame.command, fence: frame.fence });
      if (frame.command === "update_handoff" || frame.command === "stop" || frame.command === "ownership_loss") {
        rl.close();
        process.exit(0);
      }
    }
  });
  rl.on("close", () => process.exit(0));
  rl.on("error", () => process.exit(1));
}

run(parseArgs(process.argv.slice(2)).scenario);