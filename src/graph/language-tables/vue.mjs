// D2x: declarative table for vue (markup profile).

import { markupTable } from "./_factory.mjs";

export default markupTable({ id: "vue", extensions: ["vue"], grammarFile: "tree-sitter-vue.wasm" });
