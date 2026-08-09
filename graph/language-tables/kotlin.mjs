// D2x: declarative table for kotlin.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "kotlin", extensions: ["kt","kts"], grammarFile: "tree-sitter-kotlin.wasm" });
