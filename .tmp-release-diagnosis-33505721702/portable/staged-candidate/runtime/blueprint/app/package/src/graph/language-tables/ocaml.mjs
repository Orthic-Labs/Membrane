// D2x: declarative table for ocaml.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "ocaml", extensions: ["ml","mli"], grammarFile: "tree-sitter-ocaml.wasm" });
