// D32: example language table plugin for the test kit.

import { definePlugin } from "../../src/sdk/providers.mjs";

export default definePlugin({
  id: "example.lang.dsl",
  version: "1.0.0",
  protocolRange: ">=1 <2",
  type: "language-table",
  capabilities: ["declarations", "functions"],
  permissions: { filesystem: "repo-read", network: "none", process: "none" },
  hash: "example-hash",
  table: {
    id: "dsl",
    extensions: ["dsl"],
    grammarFile: "tree-sitter-dsl.wasm",
    factProfile: "code",
    functions: [{ nodeTypes: ["function_definition"], name: { field: "name" }, labels: ["Function"], container: "file" }],
  },
});
