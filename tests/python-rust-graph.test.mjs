import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { mutateManifest } from "./_store-helpers.mjs";

import {
  buildGraphGeneration,
  graphCapabilities,
  graphStatus,
  scanSourcesPublic,
  sourceCapabilityCoverage,
} from "../graph/static-provider.mjs";
import { PARSED_LANGUAGE_EXTENSIONS } from "../graph/language-extractors.mjs";
import { languageCapabilityRecords } from "../graph/language-registry.mjs";

const CORTEX = path.resolve(import.meta.dirname, "..");
const CATALOG_EXTENSIONS = languageCapabilityRecords().flatMap((record) => record.extensions);
const EXPECTED_PARSED_EXTENSIONS = [...new Set([...PARSED_LANGUAGE_EXTENSIONS, ...CATALOG_EXTENSIONS])].sort();

function withLanguageFixture(run) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "cortex-languages-"));
  try {
    fs.mkdirSync(path.join(repo, "python"));
    fs.writeFileSync(path.join(repo, "python", "service.py"), [
      "class OrderService:",
      "    def place_order(self, order):",
      "        return save_order(order)",
      "",
      "def save_order(order):",
      "    return order",
      "",
    ].join("\n"));
    fs.writeFileSync(path.join(repo, "python", "routes.py"), [
      "from .service import OrderService",
      "",
      "def route_order(order):",
      "    return OrderService().place_order(order)",
      "",
    ].join("\n"));

    fs.mkdirSync(path.join(repo, "rust"));
    fs.writeFileSync(path.join(repo, "rust", "lib.rs"), [
      "mod store;",
      "use crate::store::OrderStore;",
      "",
      "pub fn route_order(order: Order) {",
      "    OrderStore::save(order);",
      "}",
      "",
    ].join("\n"));
    fs.writeFileSync(path.join(repo, "rust", "store.rs"), [
      "pub struct OrderStore;",
      "",
      "impl OrderStore {",
      "    pub fn save(order: Order) {",
      "        persist(order);",
      "    }",
      "}",
      "",
      "fn persist(order: Order) {}",
      "",
    ].join("\n"));
    run(repo);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
}

function withFiles(files, run) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "cortex-stack-"));
  try {
    for (const [relativePath, contents] of Object.entries(files)) {
      const absolutePath = path.join(repo, relativePath);
      fs.mkdirSync(path.dirname(absolutePath), { recursive: true });
      fs.writeFileSync(absolutePath, contents);
    }
    run(repo);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
}

test("Python files emit class, method, and function symbols plus import and call edges", () => {
  withLanguageFixture((repo) => {
    const generation = buildGraphGeneration(repo);
    const method = generation.nodes.find((node) => node.id === "symbol:python/service.py::OrderService.place_order");

    assert.deepEqual(method?.evidence[0], {
      path: "python/service.py",
      startLine: 2,
      endLine: 3,
      contentHash: method?.evidence[0].contentHash,
    });
    assert.ok(generation.nodes.some((node) => node.id === "symbol:python/routes.py::route_order"));
    assert.ok(generation.nodes.some((node) => node.id === "symbol:python/service.py::save_order"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:python/routes.py"
      && edge.target === "file:python/service.py"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:python/routes.py::route_order"
      && edge.target === "symbol:python/service.py::OrderService.place_order"));
  });
});

test("Rust files emit type, impl method, and function symbols plus module and call edges", () => {
  withLanguageFixture((repo) => {
    const generation = buildGraphGeneration(repo);
    const method = generation.nodes.find((node) => node.id === "symbol:rust/store.rs::OrderStore.save");

    assert.deepEqual(method?.evidence[0], {
      path: "rust/store.rs",
      startLine: 4,
      endLine: 6,
      contentHash: method?.evidence[0].contentHash,
    });
    assert.ok(generation.nodes.some((node) => node.id === "symbol:rust/store.rs::OrderStore"));
    assert.ok(generation.nodes.some((node) => node.id === "symbol:rust/store.rs::persist"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:rust/lib.rs"
      && edge.target === "file:rust/store.rs"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:rust/lib.rs::route_order"
      && edge.target === "symbol:rust/store.rs::OrderStore.save"));
  });
});

test("capability reporting treats Python and Rust as parsed languages", () => {
  withLanguageFixture((repo) => {
    const outDir = path.join(repo, ".agent");
    buildGraphGeneration(repo, { outDir, persist: true });
    const status = graphStatus(repo, outDir);
    const parsedExtensions = graphCapabilities().languageCoverage.parsedExtensions;

    assert.ok(parsedExtensions.includes("py"));
    assert.ok(parsedExtensions.includes("rs"));
    assert.ok(status.capabilities.parsedExtensions.includes("py"));
    assert.ok(status.capabilities.parsedExtensions.includes("rs"));
    assert.ok(!status.capabilities.unsupportedExtensions.includes("py"));
    assert.ok(!status.capabilities.unsupportedExtensions.includes("rs"));
  });
});

test("provider upgrades invalidate generations that predate Python and Rust extraction", () => {
  withLanguageFixture((repo) => {
    const outDir = path.join(repo, ".agent");
    buildGraphGeneration(repo, { outDir, persist: true });
    // Age the LEXICAL layer's identity: after tree-sitter's promotion the
    // selected provider recorded in `provider` is the AST one, and it is
    // `lexicalProvider` that gates whether persisted lexical nodes are still
    // readable — which is what this test is about.
    mutateManifest(repo, (manifest) => {
      const layer = manifest.lexicalProvider ?? manifest.provider;
      layer.version = "repo-local-deterministic-v1";
      if (manifest.lexicalProvider) manifest.lexicalProvider = layer;
      else manifest.provider = layer;
    }, outDir);

    const status = graphStatus(repo, outDir);
    assert.equal(status.state, "stale");
    assert.equal(status.providerMismatch, true);
  });
});

test("React arrow components are callable symbols instead of inert constants", () => {
  withFiles({
    "src/cart.ts": "export function useCart() { return []; }\n",
    "src/Checkout.tsx": [
      "import { useCart } from './cart';",
      "export const Checkout = () => {",
      "  const cart = useCart();",
      "  return <div>{cart.length}</div>;",
      "};",
      "",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    const component = generation.nodes.find((node) => node.id === "symbol:src/Checkout.tsx::Checkout");
    assert.ok(component?.labels.includes("Function"));
    assert.ok(!component?.labels.includes("Const"));
    assert.equal(component?.evidence[0].endLine, 5);
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === component.id
      && edge.target === "symbol:src/cart.ts::useCart"));
  });
});

test("TypeScript type symbols, qualified methods, and CommonJS imports are graphed", () => {
  withFiles({
    "src/types.ts": [
      "export interface Cart { id: string }",
      "export type CartId = string;",
      "export enum CartState { Open }",
      "export async function fetchCart() { return {}; }",
      "export default class CartService {",
      "  public async loadCart() {",
      "    return fetchCart();",
      "  }",
      "}",
      "",
    ].join("\n"),
    "src/consumer.cjs": "const types = require('./types');\n",
    "src/register.mjs": "import './types';\n",
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    for (const id of [
      "symbol:src/types.ts::Cart",
      "symbol:src/types.ts::CartId",
      "symbol:src/types.ts::CartState",
      "symbol:src/types.ts::fetchCart",
      "symbol:src/types.ts::CartService.loadCart",
    ]) assert.ok(generation.nodes.some((node) => node.id === id), id);
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:src/types.ts::CartService.loadCart"
      && edge.target === "symbol:src/types.ts::fetchCart"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:src/consumer.cjs"
      && edge.target === "file:src/types.ts"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:src/register.mjs"
      && edge.target === "file:src/types.ts"));
  });
});

test("Vue and Astro component scripts participate in JavaScript symbol and import extraction", () => {
  withFiles({
    "src/state.ts": "export function loadState() { return {}; }\n",
    "src/Panel.vue": [
      "<script setup lang=\"ts\">",
      "import { loadState } from './state';",
      "const refreshPanel = () => loadState();",
      "</script>",
      "<template><button>Refresh</button></template>",
      "",
    ].join("\n"),
    "src/Page.astro": [
      "---",
      "import { loadState } from './state';",
      "function renderPage() { return loadState(); }",
      "---",
      "<main>Page</main>",
      "",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    assert.ok(generation.nodes.some((node) => node.id === "symbol:src/Panel.vue::refreshPanel"));
    assert.ok(generation.nodes.some((node) => node.id === "symbol:src/Page.astro::renderPage"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:src/Panel.vue"
      && edge.target === "file:src/state.ts"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:src/Page.astro"
      && edge.target === "file:src/state.ts"));
  });
});

test("workspace script languages emit functions and call relationships", () => {
  withFiles({
    "ops/deploy.sh": [
      "#!/usr/bin/env bash",
      "source ./helpers.sh",
      "publish_release() {",
      "  verify_release",
      "}",
      "",
    ].join("\n"),
    "ops/helpers.sh": "verify_release() { :; }\n",
    "ops/release.ps1": [
      ". ./helpers.ps1",
      "function Publish-Release {",
      "  Test-Release",
      "}",
      "",
    ].join("\n"),
    "ops/helpers.ps1": "function Test-Release { return $true }\n",
    "ops/build.cmd": [
      "@echo off",
      "call :publish_release",
      "exit /b",
      ":publish_release",
      "call :verify_release",
      "exit /b",
      ":verify_release",
      "exit /b",
      "",
    ].join("\n"),
    "ops/installer.iss": [
      "[Code]",
      "procedure PublishRelease;",
      "begin",
      "  VerifyRelease();",
      "end;",
      "procedure VerifyRelease;",
      "begin",
      "end;",
      "",
    ].join("\n"),
    "ops/tasks.vbs": [
      "Sub PublishRelease()",
      "  Call VerifyRelease()",
      "End Sub",
      "Function VerifyRelease()",
      "  VerifyRelease = True",
      "End Function",
      "",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    const expectedSymbols = [
      "symbol:ops/deploy.sh::publish_release",
      "symbol:ops/helpers.sh::verify_release",
      "symbol:ops/release.ps1::Publish-Release",
      "symbol:ops/helpers.ps1::Test-Release",
      "symbol:ops/build.cmd::publish_release",
      "symbol:ops/build.cmd::verify_release",
      "symbol:ops/installer.iss::section.Code",
      "symbol:ops/installer.iss::PublishRelease",
      "symbol:ops/installer.iss::VerifyRelease",
      "symbol:ops/tasks.vbs::PublishRelease",
      "symbol:ops/tasks.vbs::VerifyRelease",
    ];
    for (const id of expectedSymbols) assert.ok(generation.nodes.some((node) => node.id === id), id);
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:ops/deploy.sh::publish_release"
      && edge.target === "symbol:ops/helpers.sh::verify_release"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:ops/release.ps1::Publish-Release"
      && edge.target === "symbol:ops/helpers.ps1::Test-Release"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:ops/build.cmd::publish_release"
      && edge.target === "symbol:ops/build.cmd::verify_release"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:ops/installer.iss::PublishRelease"
      && edge.target === "symbol:ops/installer.iss::VerifyRelease"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:ops/tasks.vbs::PublishRelease"
      && edge.target === "symbol:ops/tasks.vbs::VerifyRelease"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:ops/deploy.sh"
      && edge.target === "file:ops/helpers.sh"));
    assert.ok(generation.edges.some((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:ops/release.ps1"
      && edge.target === "file:ops/helpers.ps1"));
  });
});

test("capabilities enumerate every parsed language in the tracked workspace stack", () => {
  assert.deepEqual(graphCapabilities().languageCoverage.parsedExtensions, EXPECTED_PARSED_EXTENSIONS);
});

test("catalog extensions and current Cortex sources have complete capability accounting", () => {
  const catalogFiles = Object.fromEntries(CATALOG_EXTENSIONS.map((extension) => [`src/sample.${extension}`, "fixture\n"]));
  withFiles(catalogFiles, (repo) => {
    const outDir = path.join(repo, ".agent");
    buildGraphGeneration(repo, { outDir, persist: true });
    assert.deepEqual(graphStatus(repo, outDir).capabilities.unsupportedExtensions, []);
  });

  const current = sourceCapabilityCoverage(scanSourcesPublic(CORTEX).files);
  assert.deepEqual(current, { unsupportedExtensions: [], unsupportedFileCount: 0 });
});

test("opaque assets and framework resource files do not masquerade as unsupported languages", () => {
  withFiles({
    "assets/icon.png": "fixture-png",
    "assets/font.ttf": "fixture-font",
    "materials/surface.mtlx": "<materialx />\n",
    "materials/scene.tres": "[gd_resource]\n",
  }, (repo) => {
    const outDir = path.join(repo, ".agent");
    buildGraphGeneration(repo, { outDir, persist: true });
    const status = graphStatus(repo, outDir);
    assert.equal(status.capabilities.unsupportedFileCount, 0);
    assert.deepEqual(status.capabilities.unsupportedExtensions, []);
  });
});

test("Swift native symbols and calls are included for first-party iOS code", () => {
  withFiles({
    "ios/Recorder.swift": [
      "struct Recorder {",
      "    func start() {",
      "        writeFrame()",
      "    }",
      "}",
      "",
      "func writeFrame() {}",
      "",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    assert.ok(generation.nodes.some((node) => node.id === "symbol:ios/Recorder.swift::Recorder"));
    assert.ok(generation.nodes.some((node) => node.id === "symbol:ios/Recorder.swift::Recorder.start"));
    assert.ok(generation.nodes.some((node) => node.id === "symbol:ios/Recorder.swift::writeFrame"));
    assert.ok(generation.edges.some((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:ios/Recorder.swift::Recorder.start"
      && edge.target === "symbol:ios/Recorder.swift::writeFrame"));
  });
});

test("vendor trees are excluded from first-party language coverage", () => {
  withFiles({
    "src/app.mts": "export function startApp() {}\n",
    "vendor/library/index.ts": "export function vendoredInternal() {}\n",
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    assert.ok(generation.nodes.some((node) => node.id === "symbol:src/app.mts::startApp"));
    assert.ok(!generation.nodes.some((node) => node.path.startsWith("vendor/")));
  });
});

test("call edges do not fan out to same-named symbols in unrelated modules", () => {
  withFiles({
    "src/selected.ts": "export function sharedAction() {}\n",
    "src/unrelated.ts": "export function sharedAction() {}\n",
    "src/route.ts": [
      "import { sharedAction } from './selected';",
      "export function runRoute() { sharedAction(); }",
      "",
    ].join("\n"),
  }, (repo) => {
    const generation = buildGraphGeneration(repo);
    const outgoing = generation.edges.filter((edge) => edge.kind === "CALLS"
      && edge.source === "symbol:src/route.ts::runRoute");
    assert.ok(outgoing.some((edge) => edge.target === "symbol:src/selected.ts::sharedAction"));
    assert.ok(!outgoing.some((edge) => edge.target === "symbol:src/unrelated.ts::sharedAction"));
  });
});
