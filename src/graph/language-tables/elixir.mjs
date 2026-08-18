// D2x: declarative table for elixir.

import { codeTable } from "./_factory.mjs";

export default codeTable({ id: "elixir", extensions: ["ex","exs"], grammarFile: "tree-sitter-elixir.wasm" });
