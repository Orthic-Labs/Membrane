import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as cases from "./mem-lifecycle-windows.mjs";

// ---------------------------------------------------------------------------
// Positive path: every exported case runs today against the live repository
// without a build, and every required MEM/LC export exists.
// ---------------------------------------------------------------------------

const REQUIRED_MEM_IDS = [
  "MEM-006", "MEM-008", "MEM-009", "MEM-010", "MEM-012", "MEM-016", "MEM-018", "MEM-019",
  "MEM-020", "MEM-021", "MEM-054", "MEM-055", "MEM-056", "MEM-057", "MEM-058", "MEM-059",
  "MEM-060", "MEM-061", "MEM-062", "MEM-063", "MEM-064", "MEM-065", "MEM-066",
];
const REQUIRED_LC_IDS = ["LC-01", "LC-02", "LC-03", "LC-04", "LC-05", "LC-06"];

test("every required MEM case export exists, is callable, and passes against the live repository", () => {
  for (const id of REQUIRED_MEM_IDS) {
    const exportName = id.replace("-", "_");
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const result = cases[exportName]();
    assert.equal(result.id, id);
    assert.equal(result.kind, "structural");
    assert.equal(result.pass, true, `${id} unexpectedly failed against the live tree: ${result.reason}`);
  }
});

test("every required LC case export exists, is callable, and passes against the live repository", () => {
  for (const id of REQUIRED_LC_IDS) {
    const exportName = id.replace("-", "_");
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const result = cases[exportName]();
    assert.equal(result.id, id);
    assert.equal(result.kind, "structural");
    assert.equal(result.pass, true, `${id} unexpectedly failed against the live tree: ${result.reason}`);
  }
});

test("every LC negative-control export exists, is callable, and passes (no forbidden pattern) against the live repository", () => {
  const ncExports = [
    "LC_01_NC_peer_drain", "LC_01_NC_stale_mutate",
    "LC_02_NC_enqueue_freshness", "LC_02_NC_hub_blocked",
    "LC_03_NC_cross_scope", "LC_03_NC_starvation",
    "LC_04_NC_dev_checkout", "LC_04_NC_incompatible_execute",
    "LC_05_NC_replay_after_tombstone", "LC_05_NC_clock_rewind",
    "LC_06_NC_enroll_outside_roots", "LC_06_NC_hook_interpreter",
  ];
  for (const name of ncExports) {
    assert.ok(typeof cases[name] === "function", `missing export ${name}`);
    const result = cases[name]();
    assert.equal(result.kind, "exclusion");
    assert.equal(result.pass, true, `${name} unexpectedly found its forbidden pattern in the live tree`);
  }
});

// ---------------------------------------------------------------------------
// Fixture helper — isolated temp root, mirroring the file layout the
// structural/exclusion checks read from. Never mutates the live repository.
// ---------------------------------------------------------------------------

function makeFixtureRoot() {
  const root = mkdtempSync(join(tmpdir(), "mem-lifecycle-windows-"));
  for (const dir of [
    "apps/membrane-tray-windows/src",
    "apps/membrane-hub/src-tauri/src",
    "engine/crates/membrane-runtime/src",
    "engine/crates/membrane-runtime/tests",
    "engine/crates/membrane-client/src",
    "engine/crates/membrane-protocol/src",
    "engine/crates/membrane-federation/src",
    "engine/crates/membrane/src",
    "engine/crates/membrane/tests",
  ]) {
    mkdirSync(join(root, dir), { recursive: true });
  }
  return root;
}

// ---------------------------------------------------------------------------
// MEM structural negative controls: canonical implementation file missing,
// and file present but carrying none of the required contract markers.
// Representative sample across the 23 MEM ids (mirrors pul-windows.test.mjs
// pattern of exercising the shared structuralCheck failure paths).
// ---------------------------------------------------------------------------

test("negative control: MEM-010 fails when serve.rs is absent", () => {
  const root = makeFixtureRoot();
  try {
    const result = cases.MEM_010({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: MEM-010 fails when serve.rs exists but carries no loopback/authentication marker", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/membrane-runtime/src/serve.rs"), "// empty stub\n", "utf8");
    const result = cases.MEM_010({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: MEM-018 fails when installation_manifest.rs is absent", () => {
  const root = makeFixtureRoot();
  try {
    const result = cases.MEM_018({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: MEM-059 fails when uninstall.rs carries no uninstall marker", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/membrane/src/uninstall.rs"), "fn noop() {}\n", "utf8");
    const result = cases.MEM_059({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: MEM-066 fails when background_review.rs omits H5/observation markers", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/membrane-runtime/src/background_review.rs"), "fn noop() {}\n", "utf8");
    const result = cases.MEM_066({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// LC structural negative controls: canonical implementation files absent.
// ---------------------------------------------------------------------------

test("negative control: LC-01 fails when residency.rs/residency_holders.rs are absent", () => {
  const root = makeFixtureRoot();
  try {
    const result = cases.LC_01({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: LC-05 fails when operations.rs/residency.rs carry no incarnation/lease marker", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/membrane-protocol/src/operations.rs"), "// empty\n", "utf8");
    writeFileSync(join(root, "engine/crates/membrane-client/src/residency.rs"), "// empty\n", "utf8");
    const result = cases.LC_05({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// LC negative controls — one executable fault-injection test per
// `negativeControl` string listed on the LC-01..LC-06 rows in
// windows-amendment-acceptance.json. Each injects the described fault into
// an isolated fixture root (never the live repository) and asserts the
// check fails and attributes the offending file. Baseline (no fault) must
// pass first, proving the check is not vacuously failing.
// ---------------------------------------------------------------------------

const LC_NEGATIVE_CONTROLS = [
  {
    row: "LC-01",
    control: "Peer-held controller drained by non-final holder fails.",
    fn: cases.LC_01_NC_peer_drain,
    file: "engine/crates/membrane-runtime/src/residency.rs",
    content: "fn force_drain_any_holder(_h: &Holder) {}\n",
  },
  {
    row: "LC-01",
    control: "Stale holder able to mutate fails.",
    fn: cases.LC_01_NC_stale_mutate,
    file: "engine/crates/membrane-runtime/src/residency.rs",
    content: "fn apply(_x: &Lease) { let allow_stale_mutation = true; }\n",
  },
  {
    row: "LC-02",
    control: "Refresh reporting freshness from enqueue rather than publication fails.",
    fn: cases.LC_02_NC_enqueue_freshness,
    file: "engine/crates/membrane-runtime/src/cli.rs",
    content: "fn refresh() { let freshness = enqueued_at; }\n",
  },
  {
    row: "LC-02",
    control: "Refresh blocked by absent Hub fails.",
    fn: cases.LC_02_NC_hub_blocked,
    file: "engine/crates/membrane-runtime/src/cli.rs",
    content: "fn hub_required_for_refresh() -> bool { true }\n",
  },
  {
    row: "LC-03",
    control: "Cross-scope evidence leak fails.",
    fn: cases.LC_03_NC_cross_scope,
    file: "engine/crates/membrane-federation/src/request.rs",
    content: "fn ignore_scope_boundary(_a: &Scope, _b: &Scope) {}\n",
  },
  {
    row: "LC-03",
    control: "Starvation beyond declared bound fails.",
    fn: cases.LC_03_NC_starvation,
    file: "engine/crates/membrane-runtime/tests/residency_holders.rs",
    content: "fn no_fairness_bound() {}\n",
  },
  {
    row: "LC-04",
    control: "Development checkout present on PATH is never used.",
    fn: cases.LC_04_NC_dev_checkout,
    file: "engine/crates/membrane/src/activation.rs",
    content: "fn use_development_checkout() {}\n",
  },
  {
    row: "LC-04",
    control: "Incompatible installed version is updated through installer, not executed.",
    fn: cases.LC_04_NC_incompatible_execute,
    file: "engine/crates/membrane/src/activation.rs",
    content: "fn execute_incompatible_installed() {}\n",
  },
  {
    row: "LC-05",
    control: "Replayed acquire after tombstone fails.",
    fn: cases.LC_05_NC_replay_after_tombstone,
    file: "engine/crates/membrane-protocol/src/operations.rs",
    content: "fn revive_closed_incarnation(_id: u128) {}\n",
  },
  {
    row: "LC-05",
    control: "Clock rewind advancing lease fails.",
    fn: cases.LC_05_NC_clock_rewind,
    file: "engine/crates/membrane-protocol/src/operations.rs",
    content: "fn accept_decreasing_authoritative_time(_t: u64) {}\n",
  },
  {
    row: "LC-06",
    control: "Enrollment writing outside installed roots fails.",
    fn: cases.LC_06_NC_enroll_outside_roots,
    file: "engine/crates/membrane/src/activation.rs",
    content: "fn enroll_arbitrary_path(_p: &str) {}\n",
  },
  {
    row: "LC-06",
    control: "Hook that executes an interpreter fails.",
    fn: cases.LC_06_NC_hook_interpreter,
    file: "engine/crates/membrane/tests/hook_containment.rs",
    content: 'fn spawn_hook() { std::process::Command::new("python").spawn().unwrap(); }\n',
  },
];

for (const nc of LC_NEGATIVE_CONTROLS) {
  test(`negative control (${nc.row}): "${nc.control}"`, () => {
    const root = makeFixtureRoot();
    try {
      const baseline = nc.fn({ root });
      assert.equal(baseline.pass, true, `${nc.row} baseline (no fixture) unexpectedly failed`);

      writeFileSync(join(root, nc.file), nc.content, "utf8");
      const faulted = nc.fn({ root });
      assert.equal(faulted.pass, false, `${nc.row} did not detect its injected fault: ${nc.control}`);
      assert.ok(
        faulted.evidence.some((e) => e.file === nc.file),
        `${nc.row} failed but did not attribute the offending file`,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
