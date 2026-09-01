// D2x: declarative table for ql (specification profile).

import { specTable } from "./_factory.mjs";

export default specTable({ id: "ql", extensions: ["ql"], grammarFile: "tree-sitter-ql.wasm" });
