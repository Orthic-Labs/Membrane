// D2x: declarative table for bash.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "bash", extensions: ["sh","bash"], grammarFile: "tree-sitter-bash.wasm" });
