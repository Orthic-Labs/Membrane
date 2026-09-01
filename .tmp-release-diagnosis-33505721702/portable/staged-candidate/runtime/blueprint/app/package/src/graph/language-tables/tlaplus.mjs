// D2x: declarative table for tlaplus (specification profile).

import { specTable } from "./_factory.mjs";

export default specTable({ id: "tlaplus", extensions: ["tla"], grammarFile: "tree-sitter-tlaplus.wasm" });
