// D2x: declarative table for rescript.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "rescript", extensions: ["res","resi"], grammarFile: "tree-sitter-rescript.wasm" });
