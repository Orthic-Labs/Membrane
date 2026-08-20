// D2x: declarative table for json (data profile).

import { dataTable } from "./_factory.mjs";

export default dataTable({ id: "json", extensions: ["json"], grammarFile: "tree-sitter-json.wasm" });
