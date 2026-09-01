// Shared table factory for the polyglot batches. Each language table encodes
// the syntax-exact facts its grammar can prove; dynamic/template/macro
// languages stay honest (ambiguous calls unresolved, data/markup profiles
// never fabricate functions/calls).

import { defineLanguageTable } from "../language-table.mjs";

const CODE_FUNCTIONS = [{ nodeTypes: ["function_definition", "function_declaration", "method_definition", "method_declaration"], name: { field: "name" }, labels: ["Function"], container: "file" }];
const CODE_CLASSES = [{ nodeTypes: ["class_definition", "class_declaration", "type_definition", "struct_definition", "interface_definition"], name: { field: "name" }, labels: ["Type"] }];
const CODE_IMPORTS = [{ nodeTypes: ["import_declaration", "import_statement", "use_declaration", "include_directive"], name: { field: null, nodeTypes: ["identifier", "scoped_identifier", "dotted_name", "string_literal"] } }];
const CODE_CALLS = [{ nodeTypes: ["call_expression", "invocation_expression", "method_invocation", "function_call"], name: { field: "function" } }];
const COMMENTS = [{ nodeTypes: ["comment", "line_comment", "block_comment"] }];

export function codeTable({ id, extensions, grammarFile, functionNodeTypes = null, classNodeTypes = null, importNodeTypes = null, callNodeTypes = null, commentNodeTypes = null }) {
  return defineLanguageTable({
    id,
    extensions,
    grammarFile,
    factProfile: "code",
    functions: functionNodeTypes ? [{ nodeTypes: functionNodeTypes, name: { field: "name" }, labels: ["Function"], container: "file" }] : CODE_FUNCTIONS,
    classes: classNodeTypes ? [{ nodeTypes: classNodeTypes, name: { field: "name" }, labels: ["Type"] }] : CODE_CLASSES,
    imports: importNodeTypes ? [{ nodeTypes: importNodeTypes, name: { field: null, nodeTypes: ["identifier", "scoped_identifier", "dotted_name", "string_literal"] } }] : CODE_IMPORTS,
    calls: callNodeTypes ? [{ nodeTypes: callNodeTypes, name: { field: "function" } }] : CODE_CALLS,
    comments: commentNodeTypes ? [{ nodeTypes: commentNodeTypes }] : COMMENTS,
    capabilities: { tier: 2 },
  });
}

export function dataTable({ id, extensions, grammarFile, nodeTypes = ["pair", "member", "mapping"] }) {
  return defineLanguageTable({
    id,
    extensions,
    grammarFile,
    factProfile: "data",
    declarations: [{ nodeTypes, name: { field: "key" }, labels: ["Key"] }],
    imports: [],
    calls: [],
    comments: [{ nodeTypes: ["comment"] }],
    capabilities: { tier: 2, profile: "data" },
  });
}

export function markupTable({ id, extensions, grammarFile }) {
  return defineLanguageTable({
    id,
    extensions,
    grammarFile,
    factProfile: "markup",
    declarations: [{ nodeTypes: ["element"], name: { field: null, nodeTypes: ["tag_name"] }, labels: ["Element"] }],
    imports: [{ nodeTypes: ["stylesheet", "script_element"], name: { field: null, nodeTypes: ["string_literal"] } }],
    calls: [],
    comments: [{ nodeTypes: ["comment"] }],
    capabilities: { tier: 2, profile: "markup" },
  });
}

export function specTable({ id, extensions, grammarFile, nodeTypes = ["declaration", "definition"] }) {
  return defineLanguageTable({
    id,
    extensions,
    grammarFile,
    factProfile: "specification",
    declarations: [{ nodeTypes, name: { field: "name" }, labels: ["Spec"] }],
    imports: [],
    calls: [],
    comments: [{ nodeTypes: ["comment"] }],
    capabilities: { tier: 2, profile: "specification" },
  });
}
