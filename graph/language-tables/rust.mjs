// D22: declarative table for Rust — function/struct/import/call facts.

import { defineLanguageTable } from "../language-table.mjs";

export default defineLanguageTable({
  id: "rust",
  extensions: ["rs"],
  grammarFile: "tree-sitter-rust.wasm",
  factProfile: "code",
  functions: [
    { nodeTypes: ["function_item"], name: { field: "name" }, labels: ["Function"], container: "file" },
  ],
  classes: [{ nodeTypes: ["struct_item", "enum_item", "trait_item"], name: { field: "name" }, labels: ["Type"] }],
  imports: [{ nodeTypes: ["use_declaration"], name: { field: null, nodeTypes: ["scoped_identifier"] } }],
  calls: [{ nodeTypes: ["call_expression"], name: { field: "function" } }],
  comments: [{ nodeTypes: ["line_comment", "block_comment"] }],
  capabilities: { tier: 1, strategy: "rust", dialect: "rust", call: { type: "call_expression", field: "function", member: "field" } },
});
