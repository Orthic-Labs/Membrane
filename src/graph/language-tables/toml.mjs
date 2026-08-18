// D2x: declarative table for toml (data profile).

import { dataTable } from "./_factory.mjs";

export default dataTable({ id: "toml", extensions: ["toml"], grammarFile: "tree-sitter-toml.wasm" });
