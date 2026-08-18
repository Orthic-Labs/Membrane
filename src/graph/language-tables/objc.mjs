// D2x: declarative table for objc.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "objc", extensions: ["m","mm"], grammarFile: "tree-sitter-objc.wasm" });
