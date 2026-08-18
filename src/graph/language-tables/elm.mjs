// D2x: declarative table for elm.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "elm", extensions: ["elm"], grammarFile: "tree-sitter-elm.wasm" });
