// Module surface extraction — the export set a JS/TS module provides and the
// named bindings other modules request from it.
//
// WHY THIS EXISTS
// ---------------
// graph/language-extractors.mjs resolves imports at FILE granularity ("which
// file does this specifier point at"). It never records WHICH NAMES crossed
// the edge, so the graph cannot answer the single most common breakage an
// editing agent causes: `import { x } from "./m"` where `./m` no longer
// exports `x`. That question needs a name-level surface on both sides.
//
// SOUNDNESS RULE (the whole design)
// ---------------------------------
// A consumer may only claim "name N is not exported by module M" when M's
// export surface is CLOSED — every export-shaped construct in M was
// recognised, and nothing in M can add names this extractor cannot see.
// Any unrecognised or dynamic construct sets an OPEN reason, and an open
// surface produces NO finding — only an omission. Under-reporting is a
// missing squiggle; over-reporting teaches an agent to "fix" working code.
//
// Extraction is AST-tier (tree-sitter), never regex, because the lexical
// layer cannot distinguish `export default function f` from `export function
// f`, cannot see through `export *`, and cannot tell code from comment.

import { loadLanguageRecord } from "./treesitter-provider.mjs";

// Extension -> tree-sitter language id. Deliberately narrow: phase 0 covers
// the JS/TS family only. Every other extension reports "unsupported", which
// suppresses findings rather than guessing at them.
const LANGUAGE_BY_EXTENSION = Object.freeze({
  ts: "typescript", mts: "typescript", cts: "typescript",
  tsx: "tsx",
  js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
});

export const SUPPORTED_SURFACE_EXTENSIONS = Object.freeze(Object.keys(LANGUAGE_BY_EXTENSION).sort());

// Every reason a module's export surface stops being enumerable. Kept as a
// closed vocabulary so `blueprint findings` can report coverage honestly and
// so a new construct cannot silently become "no finding, no explanation".
export const OPEN_SURFACE_REASONS = Object.freeze([
  "parse_error",
  "export_assignment",          // TS `export = x` — surface is one anonymous value
  "commonjs_exports",           // `module.exports = …` / `exports.x = …`
  "ambient_module",             // `declare module "x" { … }` augmentation
  "nested_export",              // export inside a namespace/module block
  "destructured_export",        // `export const { a } = …` beyond plain identifiers
  "unfollowable_star_reexport", // `export * from` a target we cannot enumerate
]);

export function extensionOf(path) {
  const value = String(path ?? "");
  const index = value.lastIndexOf(".");
  return index === -1 ? "" : value.slice(index + 1).toLowerCase();
}

export function surfaceLanguageFor(path) {
  return LANGUAGE_BY_EXTENSION[extensionOf(path)] ?? null;
}

function line(node) {
  return node.startPosition.row + 1;
}

function unquote(node) {
  const text = node?.text ?? "";
  return text.length >= 2 ? text.slice(1, -1) : text;
}

function hasToken(node, token) {
  for (const child of node.children) if (!child.isNamed && child.type === token) return true;
  return false;
}

// `export const a = 1` names `a`; `export const { a } = x` names a pattern this
// extractor refuses to guess at. Returns null to signal "open the surface".
function declaredNames(declaration) {
  const node = declaration.type === "ambient_declaration"
    ? declaration.namedChildren[0] ?? declaration
    : declaration;
  if (!node) return null;
  switch (node.type) {
    case "function_declaration":
    case "generator_function_declaration":
    case "function_signature":
    case "class_declaration":
    case "abstract_class_declaration":
    case "type_alias_declaration":
    case "interface_declaration":
    case "enum_declaration":
    case "internal_module":
    case "module": {
      const name = node.childForFieldName("name");
      return name ? [name.text] : null;
    }
    case "lexical_declaration":
    case "variable_declaration": {
      const names = [];
      for (const declarator of node.namedChildren) {
        if (declarator.type !== "variable_declarator") continue;
        const name = declarator.childForFieldName("name");
        if (!name || name.type !== "identifier") return null; // destructuring
        names.push(name.text);
      }
      return names.length ? names : null;
    }
    default:
      return null;
  }
}

function readExportStatement(statement, surface) {
  const source = statement.childForFieldName("source");
  const specifier = source ? unquote(source) : null;
  const clause = statement.namedChildren.find((child) => child.type === "export_clause");
  const namespaceExport = statement.namedChildren.find((child) => child.type === "namespace_export");
  const isDefault = hasToken(statement, "default");

  // `export = foo` — a single anonymous value, not a named surface.
  if (!source && !clause && !namespaceExport && hasToken(statement, "=")) {
    surface.open.push({ reason: "export_assignment", line: line(statement) });
    return;
  }

  // `export { a, b as c }` and `export { a } from "./m"`.
  if (clause) {
    for (const specifierNode of clause.namedChildren) {
      if (specifierNode.type !== "export_specifier") continue;
      const local = specifierNode.childForFieldName("name")?.text;
      const alias = specifierNode.childForFieldName("alias")?.text;
      if (!local) continue;
      surface.exports.push({ name: alias ?? local, line: line(specifierNode) });
      if (specifier) {
        surface.requests.push({
          kind: "reexport",
          name: local,
          localName: alias ?? local,
          specifier,
          line: line(specifierNode),
        });
      }
    }
    return;
  }

  // `export * as ns from "./m"` — contributes exactly one name.
  if (namespaceExport) {
    const name = namespaceExport.namedChildren[0]?.text ?? namespaceExport.text.replace(/^\*\s*as\s*/, "");
    if (name) surface.exports.push({ name, line: line(statement) });
    return;
  }

  // `export * from "./m"` — surface depends on the target; resolved later.
  if (source) {
    surface.starReexports.push({ specifier, line: line(statement) });
    return;
  }

  if (isDefault) {
    surface.exports.push({ name: "default", line: line(statement) });
    return;
  }

  const declaration = statement.childForFieldName("declaration");
  if (!declaration) {
    surface.open.push({ reason: "destructured_export", line: line(statement) });
    return;
  }
  const names = declaredNames(declaration);
  if (!names) {
    surface.open.push({ reason: "destructured_export", line: line(statement) });
    return;
  }
  for (const name of names) surface.exports.push({ name, line: line(statement) });
}

function readImportStatement(statement, surface) {
  const source = statement.childForFieldName("source");
  if (!source) return;
  const specifier = unquote(source);
  const clause = statement.namedChildren.find((child) => child.type === "import_clause");
  if (!clause) return; // side-effect import — requests no names

  for (const child of clause.namedChildren) {
    // `import d from "./m"` — the default binding is a real export name.
    if (child.type === "identifier") {
      surface.requests.push({ kind: "import", name: "default", localName: child.text, specifier, line: line(child) });
      continue;
    }
    // `import * as n from "./m"` — requests the whole module, no single name.
    if (child.type === "namespace_import") continue;
    if (child.type !== "named_imports") continue;
    for (const specifierNode of child.namedChildren) {
      if (specifierNode.type !== "import_specifier") continue;
      const name = specifierNode.childForFieldName("name")?.text;
      const alias = specifierNode.childForFieldName("alias")?.text;
      if (!name) continue;
      surface.requests.push({ kind: "import", name, localName: alias ?? name, specifier, line: line(specifierNode) });
    }
  }
}

// `module.exports = …`, `exports.x = …` and `declare module "x"` can all add
// names invisibly. Scanning the whole tree (not just the top level) keeps a
// conditional `module.exports` inside an `if` from being missed.
function scanForOpeningConstructs(root, surface) {
  const stack = [root];
  while (stack.length) {
    const node = stack.pop();
    if (node.type === "assignment_expression") {
      const left = node.childForFieldName("left");
      if (left?.type === "member_expression") {
        const object = left.childForFieldName("object")?.text;
        const property = left.childForFieldName("property")?.text;
        if (object === "module" && property === "exports") surface.open.push({ reason: "commonjs_exports", line: line(node) });
        else if (object === "exports") surface.open.push({ reason: "commonjs_exports", line: line(node) });
      }
      if (left?.type === "subscript_expression" && left.childForFieldName("object")?.text === "exports") {
        surface.open.push({ reason: "commonjs_exports", line: line(node) });
      }
    }
    if (node.type === "ambient_declaration" && node.namedChildren.some((child) => child.type === "module")) {
      surface.open.push({ reason: "ambient_module", line: line(node) });
    }
    if (node.type === "export_statement" && node.parent && node.parent.type !== "program") {
      surface.open.push({ reason: "nested_export", line: line(node) });
    }
    for (const child of node.namedChildren) stack.push(child);
  }
}

function countErrors(root) {
  let count = 0;
  const stack = [root];
  while (stack.length) {
    const node = stack.pop();
    if (node.type === "ERROR" || node.isMissing) count += 1;
    for (const child of node.children) stack.push(child);
  }
  return count;
}

/**
 * Extract one file's export surface and its outbound name requests.
 *
 * @param {{path: string, text: string}} file
 * @returns {Promise<{
 *   path: string, language: string|null, parseStatus: "ok"|"failed"|"unsupported",
 *   exports: Array<{name: string, line: number}>,
 *   starReexports: Array<{specifier: string, line: number}>,
 *   requests: Array<{kind: "import"|"reexport", name: string, localName: string, specifier: string, line: number}>,
 *   open: Array<{reason: string, line: number}>,
 * }>}
 */
export async function extractModuleSurface(file) {
  const path = String(file?.path ?? "");
  const language = surfaceLanguageFor(path);
  const surface = { path, language, parseStatus: "unsupported", exports: [], starReexports: [], requests: [], open: [] };
  if (!language) return surface;

  const record = await loadLanguageRecord(language);
  if (!record?.language) {
    surface.parseStatus = "failed";
    surface.open.push({ reason: "parse_error", line: 1 });
    return surface;
  }

  let tree = null;
  try {
    tree = record.parser.parse(String(file.text ?? ""));
    const root = tree.rootNode;
    const errors = countErrors(root);
    if (errors > 0) {
      // A file the grammar could not fully parse can hide anything. Report it
      // as an open surface, never as a clean one.
      surface.parseStatus = "failed";
      surface.open.push({ reason: "parse_error", line: 1 });
      return surface;
    }
    surface.parseStatus = "ok";
    for (const statement of root.namedChildren) {
      if (statement.type === "export_statement") readExportStatement(statement, surface);
      else if (statement.type === "import_statement") readImportStatement(statement, surface);
    }
    scanForOpeningConstructs(root, surface);
    return surface;
  } catch (error) {
    surface.parseStatus = "failed";
    surface.open.push({ reason: "parse_error", line: 1 });
    surface.error = String(error?.message ?? error);
    return surface;
  } finally {
    tree?.delete();
  }
}

export function surfaceIsClosed(surface) {
  return surface?.parseStatus === "ok" && surface.open.length === 0;
}
