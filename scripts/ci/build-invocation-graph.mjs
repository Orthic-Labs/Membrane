#!/usr/bin/env node
// Invocation-graph generator (migration spec N0, sections 16.2/19/19.1).
//
// Generates migration/native-rust/invocation-graph.json — the canonical
// reachability authority for runtime-language-manifest.json production_reachable
// fields. One-time legacy reconciliation is a frozen artifact, not input to
// this recurring gate.
//
// Method: static scan of tracked files (imports, requires, dynamic imports,
// process-spawn literals, repo-relative executable path literals, Rust `mod`
// trees) merged with a small curated edge set whose boundaries static analysis
// cannot observe (Hub supervisor spawn, daemon IPC). Every edge records its
// origin ("scanned" | "curated") and evidence. Unresolved references are
// recorded explicitly, never silently dropped.
//
// CLI:
//   node scripts/ci/build-invocation-graph.mjs            -> graph to stdout
//   node scripts/ci/build-invocation-graph.mjs --write    -> write artifact files
//   node scripts/ci/build-invocation-graph.mjs --json     -> summary report

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

export const GRAPH_REL = "migration/native-rust/invocation-graph.json";
export const RECONCILIATION_REL =
  "migration/native-rust/legacy-ledger-reconciliation.json";
export const MANIFEST_REL = "migration/native-rust/runtime-language-manifest.json";

const EXECUTABLE_EXTENSIONS = [".py", ".mjs", ".cjs", ".js", ".ts", ".sh", ".bash", ".cmd", ".rs"];
const INTERPRETER_EXTENSIONS = new Set([".py", ".mjs", ".cjs", ".js", ".ts"]);

function hasShebang(buf) {
  return buf.length >= 2 && buf[0] === 0x23 && buf[1] === 0x21;
}

export function isExecutableCandidate(rel, root) {
  if (EXECUTABLE_EXTENSIONS.some((e) => rel.endsWith(e))) return true;
  if (root) {
    try {
      const buf = readFileSync(join(root, rel));
      return hasShebang(buf);
    } catch {
      return false;
    }
  }
  return false;
}

export function loadTrackedFiles(root) {
  const res = spawnSync("git", ["ls-files"], { cwd: root, encoding: "utf8" });
  if (res.status !== 0) throw new Error(`git ls-files failed: ${res.stderr}`);
  return res.stdout.split("\n").filter(Boolean);
}

function currentHead(root) {
  const res = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" });
  return res.status === 0 ? res.stdout.trim() : null;
}

// ---------------------------------------------------------------------------
// Production entrypoints (curated seeds with cited justification).
// These are installed/user-reachable launch points only; reachability of every
// other file is *derived*, never asserted.
// ---------------------------------------------------------------------------

export function productionEntrypoints() {
  return [
    {
      id: "apps/membrane-hub/src-tauri/src/main.rs",
      kind: "installed-entrypoint",
      runtime: "rust",
      justification: "Hub Tauri binary main; sole resident lifecycle authority entrypoint.",
    },
    {
      id: "apps/membrane-tray-windows/src/main.rs",
      kind: "installed-entrypoint",
      runtime: "rust",
      justification: "Installed Windows tray entrypoint; supervises stable-current Hub and daemon processes.",
    },
    {
      id: "engine/crates/membrane/src/main.rs",
      kind: "installed-entrypoint",
      runtime: "rust",
      justification:
        "`membrane` binary entry (engine/crates/membrane) invoked directly by users and bundled by the Hub supervisor.",
    },
    {
      id: "blueprint/release/launchers/blueprint.cmd",
      kind: "installed-external-component-entrypoint",
      runtime: "shell",
      justification:
        "Signed Windows Blueprint CLI launcher bundled by the installer; direct consumers use bounded one-shot operations.",
    },
    {
      id: "blueprint/release/launchers/blueprint-mcp.cmd",
      kind: "installed-external-component-entrypoint",
      runtime: "shell",
      justification: "Signed Windows Blueprint MCP launcher bundled by the installer.",
    },
  ];
}

// Curated edges for boundaries static scanning cannot observe. Each cites its
// evidence symbol/path so the claim stays auditable.
export function curatedEdges() {
  return [
    {
      from: "apps/membrane-hub/src-tauri/src/main.rs",
      to: "apps/membrane-hub/src-tauri/src/supervisor.rs",
      operation: "supervise native child",
      boundary: "in-process",
      evidence: { path: "apps/membrane-hub/src-tauri/src/main.rs", symbol: "supervisor setup" },
    },
    {
      from: "engine/crates/membrane/src/main.rs",
      to: "engine/crates/membrane-runtime/src/cli.rs",
      operation: "first-class membrane adapt dispatch",
      boundary: "in-process",
      evidence: { path: "engine/crates/membrane/src/modes.rs", symbol: "dispatch_cli / membrane_runtime::cli::run_cli_from" },
    },
    {
      from: "blueprint/release/launchers/blueprint.cmd",
      to: "blueprint/scripts/blueprint.mjs",
      operation: "bounded installed Blueprint CLI launch",
      boundary: "process",
      evidence: { path: "blueprint/release/launchers/blueprint.cmd", symbol: "lib\\node.exe ... scripts\\blueprint.mjs" },
    },
    {
      from: "blueprint/release/launchers/blueprint-mcp.cmd",
      to: "blueprint/scripts/blueprint-mcp.mjs",
      operation: "bounded installed Blueprint MCP launch",
      boundary: "process",
      evidence: { path: "blueprint/release/launchers/blueprint-mcp.cmd", symbol: "lib\\node.exe ... scripts\\blueprint-mcp.mjs" },
    },
    {
      from: "blueprint/scripts/cli/commands.mjs",
      to: "blueprint/scripts/blueprint-watch.mjs",
      operation: "Hub-owned Blueprint watcher launch",
      boundary: "process",
      evidence: { path: "blueprint/scripts/cli/commands.mjs", symbol: "spawn(process.execPath, [watcherScript, \"start\"])" },
    },
    {
      from: "engine/crates/membrane-runtime/src/cli.rs",
      to: "engine/crates/membrane-adapt/src/lib.rs",
      operation: "native Adapt command handlers",
      boundary: "in-process",
      evidence: { path: "engine/crates/membrane-runtime/src/cli.rs", symbol: "run_adapt" },
    },
    {
      from: "engine/crates/membrane-runtime/src/cli.rs",
      to: "engine/crates/membrane-transcript/src/lib.rs",
      operation: "native transcript normalization for Adapt mining",
      boundary: "in-process",
      evidence: { path: "engine/crates/membrane-runtime/src/cli.rs", symbol: "run_adapt / AdaptCmd::Mine" },
    },
    {
      from: "apps/membrane-hub/src-tauri/src/supervisor.rs",
      to: "engine/crates/membrane-runtime/src/serve.rs",
      operation: "Hub-owned runtime library call",
      boundary: "in-process",
      evidence: { path: "apps/membrane-hub/src-tauri/src/supervisor.rs", symbol: "run_hub_runtime" },
    },
    {
      from: "external:host-clients",
      to: "engine/crates/membrane/src/main.rs",
      operation: "native stdio MCP session launched by generated host config",
      boundary: "stdio",
      evidence: { path: "mcp.json", symbol: "membrane stdio-mcp" },
    },
  ];
}

// ---------------------------------------------------------------------------
// Scanners
// ---------------------------------------------------------------------------

const IMPORT_PATTERNS = [
  { re: /\bimport\s+[^;'"]*?\bfrom\s+["']([^"']+)["']/g, kind: "esm-import" },
  { re: /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g, kind: "dynamic-import" },
  { re: /\brequire\s*\(\s*["']([^"']+)["']\s*\)/g, kind: "require" },
];

// Spawn calls whose first argument is process.execPath take the module path
// from a following quoted string in the same call window.
const SPAWN_EXEC_PATH_RE =
  /\b(?:spawn|spawnSync|execFile|execFileSync|fork|run)\(\s*(?:process\.execPath|NODE|node)\b[^)]{0,400}?["']([^"'\n]+\.(?:mjs|cjs|js))["']/g;
const SPAWN_LITERAL_RE =
  /\b(?:spawn|spawnSync|execFile|execFileSync|fork)\(\s*["']([^"'\n]+)["']/g;

const PY_PACKAGE_ROOTS = ["adapt/src/adapt", "continuity/transcript"];

// Boundaries static analysis may traverse when deriving production reachability.
// Weak evidence edges (boundary "path-reference") are recorded but never traversed.
export const TRAVERSABLE_BOUNDARIES = new Set([
  "in-process", "import", "module", "process", "loopback-http",
  "external-typed-protocol", "stdio", "packaged-projection",
]);

// Launch-API context that upgrades a repo-relative path literal into a strong
// process edge (spec section 19: launch sites).
export const LAUNCH_CONTEXT_RE =
  /(?:Command::new|subprocess\.|Popen|check_output|check_call|os\.system|\bspawn\w*\s*\(|\bexecFile\w*\s*\(|\bfork\s*\(|\bexec\s*\(|child_process)/;

export function resolveNodeSpecifier(spec, fromRel) {
  if (!spec.startsWith(".")) return null; // bare/node built-in: external, not a repo file
  const base = posix.join(posix.dirname(fromRel.split("\\").join("/")), spec);
  const candidates = [
    base,
    `${base}.mjs`, `${base}.cjs`, `${base}.js`, `${base}.ts`, `${base}.json`,
    posix.join(base, "index.mjs"), posix.join(base, "index.cjs"), posix.join(base, "index.js"),
  ];
  return candidates[0]; // resolution against disk happens at call site via existsSync
}

export function firstExisting(root, candidates) {
  for (const c of candidates) {
    try {
      if (statSync(join(root, c)).isFile()) return c;
    } catch {
      /* keep looking */
    }
  }
  return null;
}

export function extractRepoPathLiterals(text) {
  // Tokens anywhere in any text that look like repo-relative executables,
  // paired with whether the surrounding line shows a launch API.
  const out = [];
  const seen = new Set();
  const lines = text.split("\n");
  const re = /[\w@\-][\w@\-.\\/]*\.(?:py|mjs|cjs|sh|bash)/g;
  let m;
  lines.forEach((line) => {
    if (/include_str!|include_bytes!/.test(line)) return; // data embedding, not a launch site
    const launch = LAUNCH_CONTEXT_RE.test(line);
    while ((m = re.exec(line)) !== null) {
      let tok = m[0].split("\\").join("/");
      while (tok.startsWith("./")) tok = tok.slice(2);
      if (!tok.includes("/")) continue; // bare names are PATH-resolved -> unresolved refs, not edges
      const key = `${tok}:${launch ? "launch" : "ref"}`;
      if (!seen.has(key)) {
        seen.add(key);
        out.push({ token: tok, launch });
      }
    }
  });
  return out;
}

export function resolvePythonModule(mod, fromRel) {
  if (mod.startsWith(".")) {
    // relative import inside a package: resolve against the file's package dir
    const parts = mod.split(".");
    const depth = parts.filter((p) => p === "").length;
    const rel = parts.filter((p) => p !== "").join("/");
    let dir = posix.dirname(fromRel);
    for (let i = 1; i < depth; i++) dir = posix.dirname(dir);
    const candidates = [];
    if (rel) candidates.push(`${posix.join(dir, rel)}.py`, posix.join(dir, rel, "__init__.py"));
    return { candidates };
  }
  const segs = mod.split(".");
  const top = segs[0];
  const rest = segs.slice(1).join("/");
  // Irregular package mappings observed in the repo:
  //   python package `continuity`    -> continuity/transcript/
  //   python package `federation`    -> engine/federation/ (sys.path includes engine/)
  // Regular packages map name -> directory of the same name.
  let roots;
  if (top === "continuity") {
    // `continuity` -> continuity/transcript; `continuity.transcript.X` -> X
    const sub = rest === "" ? "" : rest.startsWith("transcript") ? rest.slice("transcript".length).replace(/^\//, "") : rest;
    const base = posix.join("continuity/transcript", sub);
    return { candidates: [`${base}.py`, posix.join(base, "__init__.py")] };
  } else if (top === "federation") {
    roots = [{ root: "engine/federation", stripTop: true }];
  } else if (top === "adapt") {
    roots = [{ root: "adapt/src/adapt", stripTop: false, self: true }];
  } else if (top === "engine") {
    roots = [{ root: "engine", stripTop: false }];
  } else if (PY_PACKAGE_ROOTS.includes(top)) {
    roots = [{ root: top, stripTop: false }];
  } else {
    roots = [];
  }
  const candidates = [];
  for (const { root: r, stripTop, self } of roots) {
    if (self && !rest) {
      candidates.push(posix.join(r, "__init__.py"));
      continue;
    }
    const base = posix.join(r, rest);
    candidates.push(`${base}.py`, posix.join(base, "__init__.py"));
  }
  return { candidates };
}

export function scanFileForEdges(root, rel, text) {
  const edges = [];
  const unresolved = [];
  const ext = posix.extname(rel);

  const addEdge = (to, operation, boundary, symbol) => {
    if (!to || to === rel) return;
    edges.push({ to, operation, boundary, symbol });
  };

  if (ext === ".py") {
    // Plain module imports: `import x.y` / `import a, b`.
    const plainRe = /^\s*import\s+([\w.]+(?:\s*,\s*[\w.]+)*)/gm;
    plainRe.lastIndex = 0;
    let m;
    while ((m = plainRe.exec(text)) !== null) {
      const mods = m[1].split(",").map((x) => x.trim()).filter(Boolean);
      for (const mod of mods) {
        const { candidates } = resolvePythonModule(mod, rel);
        const found = firstExisting(root, candidates);
        if (found) addEdge(found, "python-import", "module", mod);
        else unresolved.push({ reference: mod, kind: "python-import", reason: "module not resolved against known package roots" });
      }
    }
    // From-imports: `from pkg import name` also implies pkg/name.py.
    const fromRe = /^\s*from\s+([.\w]+)\s+import\s+([^#\n]+)/gm;
    fromRe.lastIndex = 0;
    while ((m = fromRe.exec(text)) !== null) {
      const mod = m[1];
      const names = m[2].split(",").map((x) => x.trim().split(/\s+as\s+/)[0]).filter((x) => /^[\w*]+$/.test(x));
      const base = resolvePythonModule(mod, rel).candidates;
      const foundBase = firstExisting(root, base);
      if (foundBase) addEdge(foundBase, "python-import", "module", mod);
      else unresolved.push({ reference: mod, kind: "python-from-import", reason: "module not resolved against known package roots" });
      if (!names.includes("*")) {
        for (const n of names) {
          const sub = resolvePythonModule(`${mod}.${n}`.replace(/^\.+/, (d) => d), rel).candidates;
          const foundSub = firstExisting(root, sub);
          if (foundSub && foundSub !== foundBase) addEdge(foundSub, "python-from-import", "module", `${mod}.${n}`);
        }
      }
    }
  }

  if (INTERPRETER_EXTENSIONS.has(ext)) {
    for (const { re, kind } of IMPORT_PATTERNS) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(text)) !== null) {
        const target = resolveNodeSpecifier(m[1], rel);
        if (target) {
          const found = firstExisting(root, [
            target,
            `${target}.mjs`, `${target}.cjs`, `${target}.js`, `${target}.ts`, `${target}.json`,
            posix.join(target, "index.mjs"), posix.join(target, "index.cjs"),
          ]);
          if (found) addEdge(found, kind, "import", m[1]);
          else unresolved.push({ reference: m[1], kind, reason: "specifier not present on disk" });
        }
      }
    }
    for (const re of [SPAWN_EXEC_PATH_RE, SPAWN_LITERAL_RE]) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(text)) !== null) {
        const tok = m[1].split("\\").join("/");
        const cand = tok.includes("/") ? tok : null;
        if (cand) {
          const found = firstExisting(root, [cand, posix.join(posix.dirname(rel), cand)]);
          if (found) addEdge(found, "child-process-spawn", "process", m[1]);
          else unresolved.push({ reference: m[1], kind: "spawn", reason: "target not on disk" });
        } else {
          unresolved.push({ reference: m[1], kind: "spawn", reason: "bare executable name (PATH-resolved)" });
        }
      }
    }
  }

  if (ext === ".rs") {
    // Rust module tree: `mod name;` declares sibling files.
    const lines = text.split("\n");
    lines.forEach((line) => {
      const m = line.match(/^\s*(?:pub\s+)?mod\s+(\w+)\s*;/);
      if (m) {
        const dir = posix.dirname(rel);
        const found = firstExisting(root, [
          posix.join(dir, `${m[1]}.rs`),
          posix.join(dir, m[1], "mod.rs"),
        ]);
        if (found) addEdge(found, "rust-module-declaration", "module", `mod ${m[1]};`);
      }
    });
  }

  // Repo-relative executable path literals in ANY text file (catches Rust
  // Command::new("...gateway.py"), Python subprocess lists, shell scripts,
  // generated configs referencing interpreter paths).
  // Repo-relative executable path literals in ANY text file (catches Rust
  // Command::new("...gateway.py"), Python subprocess lists, shell scripts).
  // Literals on a line showing a launch API are strong process edges; all other
  // literals are weak "path-reference" edges recorded as evidence but never
  // traversed for reachability.
  for (const { token, launch } of extractRepoPathLiterals(text)) {
    const found = firstExisting(root, [token, posix.join(posix.dirname(rel), token)]);
    if (found && found !== rel) {
      edges.push({
        to: found,
        operation: launch ? "executable-path-literal-launch" : "executable-path-reference",
        boundary: launch ? "process" : "path-reference",
        symbol: token,
      });
    }
  }

  return { edges, unresolved };
}

// ---------------------------------------------------------------------------
// Graph assembly
// ---------------------------------------------------------------------------

export function buildGraph({ root, trackedFiles, now = new Date(), head = null }) {
  const tracked = new Set(trackedFiles);
  const seeds = productionEntrypoints();
  // Honest-seed guard: an entrypoint that is not a tracked file is a generator
  // bug, not a graph fact. Fail loudly instead of inventing a node.
  for (const s of seeds) {
    if (!tracked.has(s.id)) {
      throw new Error(`production entrypoint seed is not a tracked file: ${s.id}`);
    }
  }
  const seedIds = new Set(seeds.map((s) => s.id));

  const nodes = new Map();
  const ensureNode = (id, kind, runtime) => {
    if (!nodes.has(id)) nodes.set(id, { id, kind, runtime: runtime ?? null });
    return nodes.get(id);
  };

  const edges = [];
  const unresolvedReferences = [];

  for (const f of trackedFiles) {
    if (!isExecutableCandidate(f, root)) continue;
    const ext = posix.extname(f);
    ensureNode(
      f,
      "tracked-executable",
      ext === ".rs" ? "rust" :
      ext === ".py" ? "python" :
      INTERPRETER_EXTENSIONS.has(ext) ? "node" :
      ext === ".sh" || ext === ".bash" ? "shell" : null,
    );
  }
  for (const s of seeds) ensureNode(s.id, s.kind, s.runtime);
  ensureNode("external:blueprint-daemon", "external-service", "external");
  ensureNode("external:host-clients", "external-consumer", "external");

  let scannedEdgeCount = 0;
  for (const f of trackedFiles) {
    if (!isExecutableCandidate(f, root)) continue;
    let text;
    try {
      text = readFileSync(join(root, f), "utf8");
    } catch {
      continue;
    }
    const { edges: found, unresolved } = scanFileForEdges(root, f, text);
    for (const e of found) {
      const extTo = posix.extname(e.to);
      if (!nodes.has(e.to)) {
        ensureNode(e.to, "referenced-file", extTo === ".rs" ? "rust" : INTERPRETER_EXTENSIONS.has(extTo) ? "node" : null);
      }
      edges.push({
        id: `e${edges.length}`,
        from: f,
        to: e.to,
        operation: e.operation,
        boundary: e.boundary,
        origin: "scanned",
        evidence: [{ path: f, symbol: e.symbol }],
      });
      scannedEdgeCount++;
    }
    for (const u of unresolved) unresolvedReferences.push({ from: f, ...u });
  }

  for (const e of curatedEdges()) {
    if (!nodes.has(e.to)) ensureNode(e.to, e.to.startsWith("external:") ? "external-service" : "referenced-file", null);
    edges.push({
      id: `e${edges.length}`,
      from: e.from,
      to: e.to,
      operation: e.operation,
      boundary: e.boundary,
      origin: "curated",
      evidence: [e.evidence],
    });
  }

  const crateLib = new Map(); // crate dir -> lib.rs node id
  for (const f of trackedFiles) {
    const m = f.match(/^(.*crates\/[^/]+)\/src\/lib\.rs$/);
    if (m) crateLib.set(m[1], f);
  }

  // Crate binaries link their own library: src/main.rs -> src/lib.rs.
  for (const [dir, lib] of crateLib) {
    const bin = `${dir}/src/main.rs`;
    if (trackedFiles.includes(bin)) {
      ensureNode(bin, "tracked-executable", "rust");
      edges.push({
        id: `e${edges.length}`,
        from: bin,
        to: lib,
        operation: "crate binary links its library",
        boundary: "module",
        origin: "curated",
        evidence: [{ path: bin, symbol: "lib target in same crate" }],
      });
    }
  }

  // Cross-crate Rust dependencies come from Cargo.toml path dependencies, not
  // the mod tree; add one edge per path dependency between crate roots.
  const cargoTomls = trackedFiles.filter((f) => f.endsWith("Cargo.toml"));
  for (const ct of cargoTomls) {
    let text;
    try {
      text = readFileSync(join(root, ct), "utf8");
    } catch {
      continue;
    }
    // Only [dependencies] are runtime edges; [dev-dependencies]/[build-dependencies]
    // never make a crate production-reachable.
    const depRe = /^(\w[\w-]*)\s*=\s*\{[^}]*path\s*=\s*"([^"]+)"/gm;
    const linesC = text.split(/\r?\n/);
    const lineSection = [];
    let cur = "none";
    for (const l of linesC) {
      const sec = l.match(/^\[([^\]]+)\]$/);
      if (sec) cur = sec[1].trim();
      lineSection.push(cur);
    }
    let m;
    let lineNo = -1;
    const depReGlobal = /^(\w[\w-]*)\s*=\s*\{[^}]*path\s*=\s*"([^"]+)"/;
    for (const l of linesC) {
      lineNo++;
      if (!lineSection[lineNo].startsWith("dependencies")) continue;
      m = depReGlobal.exec(l);
      if (!m) continue;
      const depName = m[1];
      const depPath = posix.normalize(posix.join(posix.dirname(ct), m[2]));
      const fromLib = crateLib.get(posix.dirname(ct));
      const toLib = crateLib.get(depPath);
      if (fromLib && toLib && fromLib !== toLib) {
        if (!nodes.has(toLib)) ensureNode(toLib, "tracked-executable", "rust");
        edges.push({
          id: `e${edges.length}`,
          from: fromLib,
          to: toLib,
          operation: `cargo path dependency ${depName}`,
          boundary: "module",
          origin: "scanned",
          evidence: [{ path: ct, symbol: `${depName} = { path = "${m[2]}" }` }],
        });
        scannedEdgeCount++;
      }
    }
  }

  // Reachability: BFS over traversable boundaries from production entrypoints.
  const adjacency = new Map();
  for (const e of edges) {
    if (!TRAVERSABLE_BOUNDARIES.has(e.boundary)) continue;
    if (!adjacency.has(e.from)) adjacency.set(e.from, []);
    adjacency.get(e.from).push(e);
  }
  const reach = new Map(); // id -> {seed, hops}
  const queue = [];
  for (const s of seedIds) {
    if (nodes.has(s)) {
      reach.set(s, { seed: s, hops: 0 });
      queue.push(s);
    }
  }
  while (queue.length) {
    const cur = queue.shift();
    const { seed, hops } = reach.get(cur);
    for (const e of adjacency.get(cur) ?? []) {
      if (!reach.has(e.to)) {
        reach.set(e.to, { seed, hops: hops + 1 });
        queue.push(e.to);
      }
    }
  }

  const nodeList = [...nodes.values()]
    .map((n) => {
      const r = reach.get(n.id);
      return {
        id: n.id,
        kind: n.kind,
        runtime: n.runtime,
        production_reachable: !!r,
        ...(r && r.hops > 0 ? { reachability_evidence: { seed: r.seed, hops: r.hops } } : {}),
      };
    })
    .sort((a, b) => a.id.localeCompare(b.id));

  const derivedProductionFiles = nodeList
    .filter((n) => n.production_reachable && n.kind === "tracked-executable")
    .map((n) => n.id);

  return {
    schemaVersion: 2,
    artifact: "membrane.invocation-graph",
    repository: "Orthic-Labs/Membrane",
    baselineCommit: head ?? currentHead(root),
    generatedAt: now.toISOString(),
    method: {
      staticScan: {
        nodeImportScan: true,
        dynamicImportScan: true,
        pythonModuleResolutionRoots: PY_PACKAGE_ROOTS.concat(["engine"]),
        childProcessLiteralScan: true,
        repoRelativeExecutablePathLiterals: true,
        rustModTree: true,
        scannedEdges: scannedEdgeCount,
      },
      curatedEdges: curatedEdges().length,
      limitations: [
        "Rust crate-internal use/reexport paths are covered via the `mod` tree only; cross-crate reexports are not separately traced.",
        "PATH-resolved bare executable names are recorded as unresolved references rather than guessed edges.",
        "Loopback HTTP calls without literal repo paths rely on the curated edge set and cite their evidence symbols.",
      ],
    },
    productionEntrypoints: seeds,
    canonicalProviderOrder: ["anchors", "blueprint", "rules", "live_files", "git", "audit", "architect", "skills", "cortex"],
    nodes: nodeList,
    edges,
    unresolvedReferences,
    nativeOwnerDecisions: [
      {
        path: "engine/crates/membrane-transcript/",
        status: trackedFiles.some((path) => path.startsWith("engine/crates/membrane-transcript/"))
          ? "adopted-workspace-member"
          : "untracked-candidate",
        ownerDecision: "N2 canonical owner: membrane-transcript. No second native transcript owner may be created beside it.",
      },
      {
        path: "engine/crates/membrane-adapt/",
        status: trackedFiles.some((path) => path.startsWith("engine/crates/membrane-adapt/"))
          ? "adopted-workspace-member"
          : "untracked-candidate",
        ownerDecision: "N3-N5 canonical owner: membrane-adapt. No second native Adapt owner may be created beside it.",
      },
    ],
    derivedProductionFiles,
    notes: [
      "production_reachable is derived here by BFS from productionEntrypoints over edges whose boundary is not 'data'; runtime-language-manifest rows must agree with this derivation (checked by check-invocation-graph.mjs).",
      "Completion covers shipped/runtime paths and their launch sites; test, evaluation, benchmark, and release-helper executables are dev-only and are not product call edges.",
    ],
  };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function main(argv) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const write = argv.includes("--write");
  const jsonOut = argv.includes("--json");

  const trackedFiles = loadTrackedFiles(root);
  const graph = buildGraph({ root, trackedFiles });

  if (jsonOut) {
    process.stdout.write(
      `${JSON.stringify(
        {
          nodes: graph.nodes.length,
          edges: graph.edges.length,
          scannedEdges: graph.method.staticScan.scannedEdges,
          curatedEdges: graph.method.curatedEdges,
          productionReachableFiles: graph.derivedProductionFiles.length,
          unresolvedReferences: graph.unresolvedReferences.length,
        },
        null,
        2,
      )}\n`,
    );
  }

  if (write) {
    writeFileSync(join(root, GRAPH_REL), `${JSON.stringify(graph, null, 1)}\n`);
    process.stdout.write(`wrote ${GRAPH_REL}\n`);
  } else if (!jsonOut) {
    process.stdout.write(`${JSON.stringify(graph, null, 1)}\n`);
  }
  return 0;
}

if (process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href) {
  process.exitCode = main(process.argv.slice(2));
}
