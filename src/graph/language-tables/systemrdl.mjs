// D2x: declarative table for systemrdl (specification profile).

import { specTable } from "./_factory.mjs";

export default specTable({ id: "systemrdl", extensions: ["rdl"], grammarFile: "tree-sitter-systemrdl.wasm" });
