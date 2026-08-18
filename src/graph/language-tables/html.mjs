// D2x: declarative table for html (markup profile).

import { markupTable } from "./_factory.mjs";

export default markupTable({ id: "html", extensions: ["html","htm"], grammarFile: "tree-sitter-html.wasm" });
