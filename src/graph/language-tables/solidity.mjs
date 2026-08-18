// D2x: declarative table for solidity.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "solidity", extensions: ["sol"], grammarFile: "tree-sitter-solidity.wasm" });
