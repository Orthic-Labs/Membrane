// D2x: declarative table for elisp.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "elisp", extensions: ["el"], grammarFile: "tree-sitter-elisp.wasm" });
