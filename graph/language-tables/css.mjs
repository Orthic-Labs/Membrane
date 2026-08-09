// D2x: declarative table for css (markup profile).

import { markupTable } from "./_factory.mjs";

export default markupTable({ id: "css", extensions: ["css"], grammarFile: "tree-sitter-css.wasm" });
