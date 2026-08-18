// D2x: declarative table for yaml (data profile).

import { dataTable } from "./_factory.mjs";

export default dataTable({ id: "yaml", extensions: ["yaml","yml"], grammarFile: "tree-sitter-yaml.wasm" });
