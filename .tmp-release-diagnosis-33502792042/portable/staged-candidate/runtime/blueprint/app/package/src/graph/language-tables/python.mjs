// D22: declarative table for Python — function/class/import/call facts.

import { defineLanguageTable } from "../language-table.mjs";

export default defineLanguageTable({
  id: "python",
  extensions: ["py"],
  grammarFile: "tree-sitter-python.wasm",
  factProfile: "code",
  functions: [
    { nodeTypes: ["function_definition"], name: { field: "name" }, labels: ["Function"], container: "file" },
  ],
  classes: [{ nodeTypes: ["class_definition"], name: { field: "name" }, labels: ["Class"] }],
  imports: [
    { nodeTypes: ["import_statement"], name: { field: null, nodeTypes: ["dotted_name"] } },
    { nodeTypes: ["import_from_statement"], name: { field: "module_name" } },
  ],
  calls: [{ nodeTypes: ["call"], name: { field: "function" } }],
  comments: [{ nodeTypes: ["comment"] }],
  capabilities: { tier: 1, strategy: "python", dialect: "python", call: { type: "call", field: "function", member: "attribute" } },
});
