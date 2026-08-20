// D2x: declarative table for zig.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "zig", extensions: ["zig"], grammarFile: "tree-sitter-zig.wasm" });
