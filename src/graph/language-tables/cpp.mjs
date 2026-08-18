// D2x: declarative table for cpp.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "cpp", extensions: ["cpp","cc","cxx","hpp"], grammarFile: "tree-sitter-cpp.wasm" });
