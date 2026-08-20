// D2x: declarative table for lua.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "lua", extensions: ["lua"], grammarFile: "tree-sitter-lua.wasm" });
