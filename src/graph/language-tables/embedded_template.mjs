// D2x: declarative table for embedded_template (markup profile).

import { markupTable } from "./_factory.mjs";

export default markupTable({ id: "embedded_template", extensions: ["erb"], grammarFile: "tree-sitter-embedded_template.wasm" });
