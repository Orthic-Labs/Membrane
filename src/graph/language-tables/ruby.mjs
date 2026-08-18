// D24: declarative table for Ruby — `def` produces `method` nodes; dynamic
// dispatch stays unresolved.

import { defineLanguageTable } from "../language-table.mjs";

export default defineLanguageTable({
  id: "ruby",
  extensions: ["rb"],
  grammarFile: "tree-sitter-ruby.wasm",
  factProfile: "code",
  functions: [{ nodeTypes: ["method", "singleton_method"], name: { field: "name" }, labels: ["Function"], container: "file" }],
  classes: [{ nodeTypes: ["class", "module"], name: { field: "name" }, labels: ["Type"] }],
  imports: [{ nodeTypes: ["call"], name: { field: null, nodeTypes: ["identifier", "constant"] } }],
  calls: [{ nodeTypes: ["call"], name: { field: "method" } }],
  comments: [{ nodeTypes: ["comment"] }],
  capabilities: { tier: 2 },
});
