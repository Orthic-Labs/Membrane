// D2x: declarative table for go.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "go", extensions: ["go"], grammarFile: "tree-sitter-go.wasm" });
