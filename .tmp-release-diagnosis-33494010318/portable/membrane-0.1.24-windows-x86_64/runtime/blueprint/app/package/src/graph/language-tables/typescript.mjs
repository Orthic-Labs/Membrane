// D22: declarative table for TypeScript — same extractor semantics as JS.

import { defineLanguageTable } from "../language-table.mjs";

export default defineLanguageTable({
  id: "typescript",
  extensions: ["ts", "mts", "cts"],
  grammarFile: "tree-sitter-typescript.wasm",
  factProfile: "code",
  functions: [
    { nodeTypes: ["function_declaration", "generator_function_declaration"], name: { field: "name" }, labels: ["Function"], container: "file" },
  ],
  classes: [{ nodeTypes: ["class_declaration"], name: { field: "name" }, labels: ["Class"] }],
  imports: [{ nodeTypes: ["import_statement"], name: { field: "source" } }],
  calls: [{ nodeTypes: ["call_expression"], name: { field: "function" } }],
  comments: [{ nodeTypes: ["comment"] }],
  capabilities: { tier: 1, strategy: "jslike", dialect: "ts", call: { type: "call_expression", field: "function", member: "property" }, typeLabels: { interface_declaration: ["Interface"], type_alias_declaration: ["TypeAlias"], enum_declaration: ["Enum"] } },
});
