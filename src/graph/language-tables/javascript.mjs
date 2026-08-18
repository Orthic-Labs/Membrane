// D22: declarative table for JavaScript — encodes existing extractor
// behavior; no new facts.

import { defineLanguageTable } from "../language-table.mjs";

export default defineLanguageTable({
  id: "javascript",
  extensions: ["js", "jsx", "mjs", "cjs"],
  grammarFile: "tree-sitter-javascript.wasm",
  factProfile: "code",
  functions: [
    { nodeTypes: ["function_declaration", "generator_function_declaration"], name: { field: "name" }, labels: ["Function"], container: "file" },
    { nodeTypes: ["arrow_function"], name: { field: null, nodeTypes: ["identifier"] }, labels: ["Function"], container: "file" },
  ],
  classes: [{ nodeTypes: ["class_declaration"], name: { field: "name" }, labels: ["Class"] }],
  imports: [{ nodeTypes: ["import_statement"], name: { field: "source" } }],
  calls: [{ nodeTypes: ["call_expression"], name: { field: "function" } }],
  comments: [{ nodeTypes: ["comment"] }],
  capabilities: { tier: 1, strategy: "jslike", dialect: "js", call: { type: "call_expression", field: "function", member: "property" } },
});
