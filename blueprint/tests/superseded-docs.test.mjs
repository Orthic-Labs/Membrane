import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CLI = path.resolve(HERE, "../scripts/cortex.mjs");

function write(repo, relativePath, body) {
  const target = path.join(repo, relativePath);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, body);
}

test("superseded and archived docs remain mapped but leave live Cortex inputs", () => {
  const repo = path.join(os.tmpdir(), `cortex-superseded-${process.pid}-${Date.now()}`);
  fs.mkdirSync(repo, { recursive: true });
  try {
    write(repo, "README.md", "# Current product\n\nThe current product is implemented.\n");
    write(repo, "docs/current.md", "# Current decision\n\nThe current gate is implemented.\n");
    write(
      repo,
      "docs/old-plan.md",
      "> Superseded by `docs/current.md` on 2026-07-13.\n" +
        "> Retained as historical context; do not treat as current.\n\n" +
        "# Old plan\n\nThe legacy gate is implemented by `src/missing.ts`.\n",
    );
    write(
      repo,
      "docs/old-site-audit.md",
      "> Superseded on 2026-06-28.\n" +
        "> Historical audit of a retired site snapshot; the live site is audited separately outside this repository.\n\n" +
        "# Old site audit\n\nThe public sitemap is implemented by `public/missing.ts`.\n",
    );
    write(
      repo,
      "docs/archive/retired.md",
      "# Retired notes\n\nThe archived implementation is in `legacy/missing.ts`.\n",
    );
    write(repo, "src/current.ts", "export const current = true;\n");

    const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--no-readme-link"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);

    const map = JSON.parse(fs.readFileSync(path.join(repo, ".agent/map.json"), "utf8"));
    const claims = JSON.parse(fs.readFileSync(path.join(repo, ".agent/claims.json"), "utf8"));
    const stale = JSON.parse(fs.readFileSync(path.join(repo, ".agent/stale.json"), "utf8"));
    const queue = JSON.parse(fs.readFileSync(path.join(repo, ".agent/queue.json"), "utf8"));
    const docs = new Map(map.nodes.filter((node) => node.kind === "doc").map((node) => [node.path, node]));

    assert.deepEqual(docs.get("docs/old-plan.md")?.lifecycle, {
      status: "superseded",
      supersededBy: "docs/current.md",
      supersededOn: "2026-07-13",
    });
    assert.deepEqual(docs.get("docs/old-site-audit.md")?.lifecycle, {
      status: "superseded",
      supersededBy: null,
      supersededOn: "2026-06-28",
    });
    assert.deepEqual(docs.get("docs/archive/retired.md")?.lifecycle, {
      status: "archived",
      supersededBy: null,
      supersededOn: null,
    });

    const historicalSources = new Set([
      "docs/old-plan.md",
      "docs/old-site-audit.md",
      "docs/archive/retired.md",
    ]);
    assert.ok(claims.every((claim) => !historicalSources.has(claim.source)));
    assert.ok(queue.claims.every((claim) => !historicalSources.has(claim.source)));
    assert.ok(stale.missingReferences.every((item) => !historicalSources.has(item.source)));
    assert.ok(map.edges.some((edge) => (
      edge.type === "supersedes"
      && edge.from === docs.get("docs/current.md")?.id
      && edge.to === docs.get("docs/old-plan.md")?.id
    )));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("an inline superseded note does not retire an otherwise current document", () => {
  const repo = path.join(os.tmpdir(), `cortex-inline-superseded-${process.pid}-${Date.now()}`);
  fs.mkdirSync(repo, { recursive: true });
  try {
    write(
      repo,
      "README.md",
      "# Current reference\n\nThe current setting is implemented.\n\n" +
        "> Superseded by `src/current.ts` on 2026-07-13.\n" +
        "> This note applies only to the preceding setting.\n",
    );
    write(repo, "src/current.ts", "export const current = true;\n");

    const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--no-readme-link"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const map = JSON.parse(fs.readFileSync(path.join(repo, ".agent/map.json"), "utf8"));
    const readme = map.nodes.find((node) => node.kind === "doc" && node.path === "README.md");
    const claims = JSON.parse(fs.readFileSync(path.join(repo, ".agent/claims.json"), "utf8"));

    assert.equal(readme?.lifecycle?.status, "current");
    assert.ok(claims.some((claim) => claim.source === "README.md"));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("malformed top-level supersession banners stay current and surface an invalid marker", () => {
  const repo = path.join(os.tmpdir(), `cortex-malformed-superseded-${process.pid}-${Date.now()}`);
  fs.mkdirSync(repo, { recursive: true });
  try {
    write(
      repo,
      "README.md",
      "> Superseded by `docs/current.md` on 2026-07-13.\n" +
        "> Kept for reference.\n\n" +
        "# Current reference\n\nThe current setting is implemented.\n",
    );
    write(
      repo,
      "docs/external.md",
      "> Superseded on 2026-06-28.\n\n" +
        "# Historical site audit\n\nThe old sitemap is implemented.\n",
    );
    write(repo, "docs/current.md", "# Current decision\n\nThe current gate is implemented.\n");

    const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--no-readme-link"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const map = JSON.parse(fs.readFileSync(path.join(repo, ".agent/map.json"), "utf8"));
    const claims = JSON.parse(fs.readFileSync(path.join(repo, ".agent/claims.json"), "utf8"));
    const stale = JSON.parse(fs.readFileSync(path.join(repo, ".agent/stale.json"), "utf8"));
    const docs = new Map(map.nodes.filter((node) => node.kind === "doc").map((node) => [node.path, node]));

    assert.equal(docs.get("README.md")?.lifecycle?.status, "current");
    assert.equal(docs.get("docs/external.md")?.lifecycle?.status, "current");
    assert.ok(claims.some((claim) => claim.source === "README.md"));
    assert.ok(claims.some((claim) => claim.source === "docs/external.md"));
    assert.deepEqual(stale.invalidSupersessionMarkers, [
      {
        source: "README.md",
        target: "docs/current.md",
        reason: "historical-note-invalid",
      },
      {
        source: "docs/external.md",
        reason: "historical-note-invalid",
      },
    ]);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("a supersession marker cannot hide claims through a path outside the repository", () => {
  const parent = path.join(os.tmpdir(), `cortex-superseded-escape-${process.pid}-${Date.now()}`);
  const repo = path.join(parent, "repo");
  fs.mkdirSync(repo, { recursive: true });
  try {
    write(parent, "outside.md", "# Unrelated file\n");
    write(
      repo,
      "README.md",
      "> Superseded by `../outside.md` on 2026-07-13.\n" +
        "> Retained as historical context; do not treat as current.\n\n" +
        "# Current reference\n\nThe current setting is implemented.\n",
    );

    const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--no-readme-link"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const map = JSON.parse(fs.readFileSync(path.join(repo, ".agent/map.json"), "utf8"));
    const readme = map.nodes.find((node) => node.kind === "doc" && node.path === "README.md");
    const claims = JSON.parse(fs.readFileSync(path.join(repo, ".agent/claims.json"), "utf8"));
    const stale = JSON.parse(fs.readFileSync(path.join(repo, ".agent/stale.json"), "utf8"));

    assert.equal(readme?.lifecycle?.status, "current");
    assert.ok(claims.some((claim) => claim.source === "README.md"));
    assert.deepEqual(stale.invalidSupersessionMarkers, [{
      source: "README.md",
      target: "../outside.md",
      reason: "target-outside-repo",
    }]);
  } finally {
    fs.rmSync(parent, { recursive: true, force: true });
  }
});

test("an absolute supersession target is rejected before filesystem containment checks", () => {
  const parent = path.join(os.tmpdir(), `cortex-superseded-absolute-${process.pid}-${Date.now()}`);
  const repo = path.join(parent, "repo");
  const outside = path.join(parent, "outside.md").replaceAll("\\", "/");
  fs.mkdirSync(repo, { recursive: true });
  try {
    write(parent, "outside.md", "# Unrelated file\n");
    write(
      repo,
      "README.md",
      `> Superseded by \`${outside}\` on 2026-07-13.\n` +
        "> Retained as historical context; do not treat as current.\n\n" +
        "# Current reference\n\nThe current setting is implemented.\n",
    );

    const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--no-readme-link"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const stale = JSON.parse(fs.readFileSync(path.join(repo, ".agent/stale.json"), "utf8"));
    assert.deepEqual(stale.invalidSupersessionMarkers, [{
      source: "README.md",
      target: outside,
      reason: "target-absolute",
    }]);
  } finally {
    fs.rmSync(parent, { recursive: true, force: true });
  }
});

test("a symlinked supersession target cannot escape the repository", (t) => {
  const parent = path.join(os.tmpdir(), `cortex-superseded-symlink-${process.pid}-${Date.now()}`);
  const repo = path.join(parent, "repo");
  fs.mkdirSync(path.join(repo, "docs"), { recursive: true });
  try {
    write(parent, "outside.md", "# Unrelated file\n");
    try {
      fs.symlinkSync(path.join(parent, "outside.md"), path.join(repo, "docs/linked.md"), "file");
    } catch (error) {
      if (["EPERM", "EACCES", "ENOTSUP"].includes(error?.code)) {
        t.skip(`file symlinks unavailable: ${error.code}`);
        return;
      }
      throw error;
    }
    write(
      repo,
      "README.md",
      "> Superseded by `docs/linked.md` on 2026-07-13.\n" +
        "> Retained as historical context; do not treat as current.\n\n" +
        "# Current reference\n\nThe current setting is implemented.\n",
    );

    const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--no-readme-link"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const map = JSON.parse(fs.readFileSync(path.join(repo, ".agent/map.json"), "utf8"));
    const readme = map.nodes.find((node) => node.kind === "doc" && node.path === "README.md");
    const stale = JSON.parse(fs.readFileSync(path.join(repo, ".agent/stale.json"), "utf8"));

    assert.equal(readme?.lifecycle?.status, "current");
    assert.deepEqual(stale.invalidSupersessionMarkers, [{
      source: "README.md",
      target: "docs/linked.md",
      reason: "target-outside-repo",
    }]);
  } finally {
    fs.rmSync(parent, { recursive: true, force: true });
  }
});
