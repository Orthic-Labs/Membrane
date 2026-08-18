import { SCHEMA_EXTENSIONS, extractSchemaSymbols } from "./schema-extractors.mjs";

const JAVASCRIPT_EXTENSIONS = new Set(["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"]);
const COMPONENT_EXTENSIONS = new Set(["vue", "astro"]);
const SCRIPT_EXTENSIONS = new Set(["sh", "ps1", "bat", "cmd", "vbs"]);
const NATIVE_EXTENSIONS = new Set(["c", "h", "cc", "cpp", "cxx", "hpp", "m", "mm"]);
const NSIS_EXTENSIONS = new Set(["nsi", "nsh"]);
const INNO_SETUP_EXTENSIONS = new Set(["iss"]);

export const CODE_EXTENSIONS = new Set([
  ...JAVASCRIPT_EXTENSIONS,
  ...COMPONENT_EXTENSIONS,
  ...SCRIPT_EXTENSIONS,
  ...NATIVE_EXTENSIONS,
  ...NSIS_EXTENSIONS,
  ...INNO_SETUP_EXTENSIONS,
  ...SCHEMA_EXTENSIONS,
  "py",
  "rs",
  "swift",
]);
export const PARSED_LANGUAGE_EXTENSIONS = [...CODE_EXTENSIONS].sort();

export function extractSymbols(file, addNode) {
  const extension = file.path.split(".").at(-1)?.toLowerCase();
  if (SCHEMA_EXTENSIONS.has(extension)) return extractSchemaSymbols(file, addNode);
  if (extension === "py") return extractPythonSymbols(file, addNode);
  if (extension === "rs") return extractRustSymbols(file, addNode);
  if (extension === "swift") return extractSwiftSymbols(file, addNode);
  if (NATIVE_EXTENSIONS.has(extension)) return extractNativeSymbols(file, addNode);
  if (NSIS_EXTENSIONS.has(extension)) return extractNsisSymbols(file, addNode);
  if (INNO_SETUP_EXTENSIONS.has(extension)) return extractInnoSetupSymbols(file, addNode);
  if (SCRIPT_EXTENSIONS.has(extension)) return extractScriptSymbols(file, extension, addNode);
  return extractJavaScriptSymbols(file, addNode);
}

function extractJavaScriptSymbols(file, addNode) {
  const classStack = [];
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const lineNo = index + 1;
    const classMatch = line.match(/^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)/);
    if (classMatch) {
      const name = classMatch[1];
      const endLine = findBlockEnd(file.lines, index);
      classStack.push({ name, endLine });
      addNode(symbolNode("class", file, name, name, lineNo, endLine));
      continue;
    }
    while (classStack.length && lineNo > classStack.at(-1).endLine) classStack.pop();
    const functionMatch = line.match(/^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(/);
    if (functionMatch) {
      addNode(symbolNode("symbol", file, functionMatch[1], functionMatch[1], lineNo, findBlockEnd(file.lines, index), ["Function"]));
    }
    const arrowMatch = line.match(/^\s*(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*=>/);
    if (arrowMatch) {
      addNode(symbolNode("symbol", file, arrowMatch[1], arrowMatch[1], lineNo, findBlockEnd(file.lines, index), ["Function"]));
    }
    const constMatch = !arrowMatch && line.match(/^\s*(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\b/);
    if (constMatch) {
      addNode(symbolNode("symbol", file, constMatch[1], constMatch[1], lineNo, lineNo, ["Const"]));
    }
    const typeMatch = line.match(/^\s*(?:export\s+)?(?:declare\s+)?(interface|type|enum)\s+([A-Za-z_$][\w$]*)\b/);
    if (typeMatch) {
      const [, typeKind, name] = typeMatch;
      const label = typeKind === "type" ? "TypeAlias" : `${typeKind[0].toUpperCase()}${typeKind.slice(1)}`;
      addNode(symbolNode("symbol", file, name, name, lineNo, typeKind === "type" ? lineNo : findBlockEnd(file.lines, index), [label]));
    }
    const currentClass = classStack.at(-1);
    const methodMatch = currentClass && line.match(/^\s{2,}(?:(?:public|private|protected|static|abstract|async|readonly|override|get|set)\s+)*([A-Za-z_$][\w$]*)\s*(?:<[^>]+>)?\s*\([^)]*\)\s*(?::[^\{]+)?\s*\{/);
    const nonMethods = new Set(["constructor", "if", "for", "while", "switch", "catch"]);
    if (methodMatch && !nonMethods.has(methodMatch[1])) {
      const qualified = `${currentClass.name}.${methodMatch[1]}`;
      addNode(symbolNode("symbol", file, qualified, qualified, lineNo, findBlockEnd(file.lines, index), ["Method"]));
    }
    const testMatch = line.match(/^\s*test\s*\(\s*["']([^"']+)["']/);
    if (testMatch) {
      addNode(symbolNode("symbol", file, testMatch[1], testMatch[1], lineNo, findBlockEnd(file.lines, index), ["Test"]));
    }
  }
}

function extractPythonSymbols(file, addNode) {
  const classStack = [];
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const lineNo = index + 1;
    const indent = leadingWhitespace(line);
    while (classStack.length && indent <= classStack.at(-1).indent) classStack.pop();
    const classMatch = line.match(/^\s*class\s+([A-Za-z_]\w*)\b/);
    if (classMatch) {
      const name = classMatch[1];
      const parent = classStack.map((entry) => entry.name).join(".");
      const qualified = parent ? `${parent}.${name}` : name;
      addNode(symbolNode("class", file, name, qualified, lineNo, findPythonBlockEnd(file.lines, index), ["Class"]));
      classStack.push({ name, indent });
      continue;
    }
    const functionMatch = line.match(/^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\(/);
    if (!functionMatch) continue;
    const name = functionMatch[1];
    const currentClass = classStack.at(-1);
    const qualified = currentClass ? `${classStack.map((entry) => entry.name).join(".")}.${name}` : name;
    addNode(symbolNode("symbol", file, name, qualified, lineNo, findPythonBlockEnd(file.lines, index), [currentClass ? "Method" : "Function"]));
  }
}

function extractRustSymbols(file, addNode) {
  const implStack = [];
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const lineNo = index + 1;
    while (implStack.length && lineNo > implStack.at(-1).endLine) implStack.pop();
    const typeMatch = line.match(/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(struct|enum|trait)\s+([A-Za-z_]\w*)\b/);
    if (typeMatch) {
      const [, typeKind, name] = typeMatch;
      addNode(symbolNode("class", file, name, name, lineNo, findBlockEnd(file.lines, index), ["Class", titleCase(typeKind)]));
    }
    const implMatch = line.match(/^\s*impl(?:\s*<[^>]+>)?\s+(?:(?:[A-Za-z_]\w*(?:::\w+)*)(?:<[^>]+>)?\s+for\s+)?([A-Za-z_]\w*(?:::\w+)*)(?:<[^>]+>)?\s*\{/);
    if (implMatch) {
      implStack.push({ name: implMatch[1].split("::").at(-1), endLine: findBlockEnd(file.lines, index) });
      continue;
    }
    const functionMatch = line.match(/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+(?:"[^"]+"\s+)?)?fn\s+([A-Za-z_]\w*)\s*(?:<[^>]+>)?\s*\(/);
    if (!functionMatch) continue;
    const name = functionMatch[1];
    const currentImpl = implStack.at(-1);
    const qualified = currentImpl ? `${currentImpl.name}.${name}` : name;
    addNode(symbolNode("symbol", file, name, qualified, lineNo, findBlockEnd(file.lines, index), [currentImpl ? "Method" : "Function"]));
  }
}

function extractSwiftSymbols(file, addNode) {
  const typeStack = [];
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const lineNo = index + 1;
    while (typeStack.length && lineNo > typeStack.at(-1).endLine) typeStack.pop();
    const modifiers = "(?:(?:public|private|internal|fileprivate|open|final|indirect|nonisolated)\\s+)*";
    const typeMatch = line.match(new RegExp(`^\\s*${modifiers}(class|struct|enum|protocol|actor)\\s+([A-Za-z_]\\w*)\\b`));
    if (typeMatch) {
      const [, typeKind, name] = typeMatch;
      const endLine = findBlockEnd(file.lines, index);
      addNode(symbolNode("class", file, name, name, lineNo, endLine, ["Class", titleCase(typeKind)]));
      typeStack.push({ name, endLine });
      continue;
    }
    const extensionMatch = line.match(/^\s*extension\s+([A-Za-z_]\w*)\b[^\{]*\{/);
    if (extensionMatch) {
      typeStack.push({ name: extensionMatch[1], endLine: findBlockEnd(file.lines, index) });
      continue;
    }
    const functionMatch = line.match(/^\s*(?:(?:public|private|internal|fileprivate|open|final|static|class|mutating|nonmutating|override|required|convenience|nonisolated)\s+)*func\s+([A-Za-z_]\w*)\s*(?:<[^>]+>)?\s*\(/);
    if (!functionMatch) continue;
    const name = functionMatch[1];
    const currentType = typeStack.at(-1);
    const qualified = currentType ? `${currentType.name}.${name}` : name;
    addNode(symbolNode("symbol", file, name, qualified, lineNo, findBlockEnd(file.lines, index), [currentType ? "Method" : "Function"]));
  }
}

function extractScriptSymbols(file, extension, addNode) {
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const lineNo = index + 1;
    if (extension === "sh") {
      const match = line.match(/^\s*(?:function\s+)?([A-Za-z_][\w-]*)\s*(?:\(\s*\))?\s*\{/);
      if (match) addNode(symbolNode("symbol", file, match[1], match[1], lineNo, findBlockEnd(file.lines, index), ["Function"]));
    } else if (extension === "ps1") {
      const match = line.match(/^\s*function\s+([A-Za-z_][\w-]*)\b[^\{]*\{/i);
      if (match) addNode(symbolNode("symbol", file, match[1], match[1], lineNo, findBlockEnd(file.lines, index), ["Function"]));
    } else if (extension === "bat" || extension === "cmd") {
      const match = line.match(/^:([A-Za-z_][\w.-]*)\s*$/);
      if (match) addNode(symbolNode("symbol", file, match[1], match[1], lineNo, findBatchBlockEnd(file.lines, index), ["Function"]));
    } else if (extension === "vbs") {
      const match = line.match(/^\s*(?:(?:Public|Private)\s+)?(?:Sub|Function)\s+([A-Za-z_]\w*)\b/i);
      if (match) addNode(symbolNode("symbol", file, match[1], match[1], lineNo, findVbsBlockEnd(file.lines, index), ["Function"]));
    }
  }
}

function extractNativeSymbols(file, addNode) {
  let objectiveCType = null;
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const lineNo = index + 1;
    const objectiveCStart = line.match(/^\s*@implementation\s+([A-Za-z_]\w*)/);
    if (objectiveCStart) { objectiveCType = objectiveCStart[1]; continue; }
    if (/^\s*@end\b/.test(line)) { objectiveCType = null; continue; }
    const objectiveCMethod = objectiveCType && line.match(/^\s*[-+]\s*\([^)]*\)\s*([A-Za-z_]\w*)[^\{]*\{/);
    if (objectiveCMethod) {
      const qualified = `${objectiveCType}.${objectiveCMethod[1]}`;
      addNode(symbolNode("symbol", file, objectiveCMethod[1], qualified, lineNo, findBlockEnd(file.lines, index), ["Method"]));
      continue;
    }
    const typeMatch = line.match(/^\s*(?:typedef\s+)?(class|struct|enum)\s+([A-Za-z_]\w*)\b/);
    if (typeMatch) {
      addNode(symbolNode("class", file, typeMatch[2], typeMatch[2], lineNo, findBlockEnd(file.lines, index), ["Class", titleCase(typeMatch[1])]));
    }
    const functionMatch = line.match(/^\s*(?:(?:static|inline|extern|virtual|constexpr|const|unsigned|signed)\s+)*(?:[A-Za-z_]\w*(?:::\w+)?(?:\s*[*&])?\s+)+([A-Za-z_]\w*(?:::\w+)*)\s*\([^;{}]*\)\s*([;{])/);
    if (!functionMatch) continue;
    const qualified = functionMatch[1].replaceAll("::", ".");
    const name = qualified.split(".").at(-1);
    addNode(symbolNode("symbol", file, name, qualified, lineNo, functionMatch[2] === ";" ? lineNo : findBlockEnd(file.lines, index), ["Function"]));
  }
}

function extractNsisSymbols(file, addNode) {
  for (let index = 0; index < file.lines.length; index += 1) {
    const functionMatch = file.lines[index].match(/^\s*Function\s+([A-Za-z_.][\w.-]*)\s*$/i);
    const sectionMatch = file.lines[index].match(/^\s*Section\s+(?:"[^"]*"\s+)?([A-Za-z_.][\w.-]*)\s*$/i);
    const match = functionMatch ?? sectionMatch;
    if (!match) continue;
    const endToken = functionMatch ? "FunctionEnd" : "SectionEnd";
    addNode(symbolNode("symbol", file, match[1], match[1], index + 1, findNamedEnd(file.lines, index, endToken), [functionMatch ? "Function" : "Section"]));
  }
}

function extractInnoSetupSymbols(file, addNode) {
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index];
    const section = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (section) {
      addNode(symbolNode("symbol", file, section[1], `section.${section[1]}`, index + 1, index + 1, ["Section"]));
      continue;
    }
    const routine = line.match(/^\s*(?:procedure|function)\s+([A-Za-z_]\w*)\b/i);
    if (routine) addNode(symbolNode("symbol", file, routine[1], routine[1], index + 1, findPascalRoutineEnd(file.lines, index), ["Function"]));
  }
}

export function extractImports(file, files) {
  const extension = file.path.split(".").at(-1)?.toLowerCase();
  if (SCHEMA_EXTENSIONS.has(extension)) return [];
  if (extension === "py") return extractPythonImports(file, files);
  if (extension === "rs") return extractRustImports(file, files);
  if (NATIVE_EXTENSIONS.has(extension)) return extractNativeImports(file, files);
  if (NSIS_EXTENSIONS.has(extension)) return extractNsisImports(file, files);
  if (INNO_SETUP_EXTENSIONS.has(extension)) return extractInnoSetupImports(file, files);
  if (["sh", "ps1", "bat", "cmd"].includes(extension)) return extractScriptImports(file, files, extension);
  return extractJavaScriptImports(file, files);
}

// Companion to extractImports: returns the RELATIVE import specifiers that
// looked like a real repo-local import but resolved to no file (cortex
// B3 — unresolved references must be tagged, never silently dropped).
// extractImports() only ever returns specifiers that DID resolve, so this
// walks the same relative-specifier candidates and reports the misses.
// Scoped to the two dialects with test coverage (JS/TS, Python); other
// dialects (Rust/native/script/nsis) are honestly reported as
// "not yet covered" by the caller rather than guessed at here.
export function extractUnresolvedImportSpecifiers(file, files) {
  const extension = file.path.split(".").at(-1)?.toLowerCase();
  if (SCHEMA_EXTENSIONS.has(extension)) return [];
  if (extension === "py") return extractUnresolvedPythonImports(file, files);
  if (JAVASCRIPT_EXTENSIONS.has(extension) || COMPONENT_EXTENSIONS.has(extension)) {
    return extractUnresolvedJavaScriptImports(file, files);
  }
  return [];
}

function extractUnresolvedJavaScriptImports(file, files) {
  const unresolved = new Set();
  const baseDir = file.path.split("/").slice(0, -1);
  const specifiers = [
    ...[...file.text.matchAll(/(?:import|export)(?:\s+type)?[\s\S]*?\sfrom\s+["']([^"']+)["']/g)].map((match) => match[1]),
    ...[...file.text.matchAll(/import\s*["']([^"']+)["']/g)].map((match) => match[1]),
    ...[...file.text.matchAll(/require\s*\(\s*["']([^"']+)["']\s*\)/g)].map((match) => match[1]),
  ];
  for (const specifier of specifiers) {
    if (!specifier.startsWith(".")) continue;
    const raw = [...baseDir, ...specifier.split("/")].filter((part) => part && part !== ".");
    const parts = [];
    for (const part of raw) part === ".." ? parts.pop() : parts.push(part);
    const base = parts.join("/").replace(/\.(js|jsx|mjs|cjs)$/, "");
    const candidates = [
      base,
      ...["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "vue", "astro"].map((extension) => `${base}.${extension}`),
      ...["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"].map((extension) => `${base}/index.${extension}`),
    ];
    const found = files.some((candidate) => candidates.includes(candidate.path));
    if (!found) unresolved.add(specifier);
  }
  return [...unresolved];
}

function extractUnresolvedPythonImports(file, files) {
  const unresolved = new Set();
  const baseDir = file.path.split("/").slice(0, -1);
  for (const line of file.lines) {
    const fromMatch = line.match(/^\s*from\s+([.A-Za-z_]\w*(?:\.\w+)*)\s+import\s+/);
    const importMatch = line.match(/^\s*import\s+([A-Za-z_]\w*(?:\.\w+)*)/);
    const moduleName = fromMatch?.[1] ?? importMatch?.[1];
    if (!moduleName) continue;
    const leadingDots = moduleName.match(/^\.+/)?.[0].length ?? 0;
    // Only a relative import (leading dots) is confidently "repo-local" —
    // a bare `import os` has no expectation of resolving to a repo file, so
    // treating it as unresolved would be a false claim, not an honest one.
    if (leadingDots === 0) continue;
    const moduleParts = moduleName.slice(leadingDots).split(".").filter(Boolean);
    const relativeBase = baseDir.slice(0, Math.max(0, baseDir.length - (leadingDots - 1)));
    const candidateBase = [...relativeBase, ...moduleParts].join("/");
    const candidates = [`${candidateBase}.py`, `${candidateBase}/__init__.py`, ...suffixModuleCandidates(files, moduleParts, "py")];
    const found = candidates.some((candidate) => files.some((item) => item.path === candidate));
    if (!found) unresolved.add(moduleName);
  }
  return [...unresolved];
}

function extractJavaScriptImports(file, files) {
  const imports = new Set();
  const baseDir = file.path.split("/").slice(0, -1);
  const specifiers = [
    ...[...file.text.matchAll(/(?:import|export)(?:\s+type)?[\s\S]*?\sfrom\s+["']([^"']+)["']/g)].map((match) => match[1]),
    ...[...file.text.matchAll(/import\s*["']([^"']+)["']/g)].map((match) => match[1]),
    ...[...file.text.matchAll(/require\s*\(\s*["']([^"']+)["']\s*\)/g)].map((match) => match[1]),
  ];
  for (const specifier of specifiers) {
    if (!specifier.startsWith(".")) continue;
    const raw = [...baseDir, ...specifier.split("/")].filter((part) => part && part !== ".");
    const parts = [];
    for (const part of raw) part === ".." ? parts.pop() : parts.push(part);
    const base = parts.join("/").replace(/\.(js|jsx|mjs|cjs)$/, "");
    const candidates = [
      base,
      ...["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "vue", "astro"].map((extension) => `${base}.${extension}`),
      ...["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"].map((extension) => `${base}/index.${extension}`),
    ];
    const found = files.find((candidate) => candidates.includes(candidate.path));
    if (found) imports.add(found.path);
  }
  return [...imports];
}

function extractPythonImports(file, files) {
  const imports = new Set();
  const baseDir = file.path.split("/").slice(0, -1);
  for (const line of file.lines) {
    const fromMatch = line.match(/^\s*from\s+([.A-Za-z_]\w*(?:\.\w+)*)\s+import\s+/);
    const importMatch = line.match(/^\s*import\s+([A-Za-z_]\w*(?:\.\w+)*)/);
    const moduleName = fromMatch?.[1] ?? importMatch?.[1];
    if (!moduleName) continue;
    const leadingDots = moduleName.match(/^\.+/)?.[0].length ?? 0;
    const moduleParts = moduleName.slice(leadingDots).split(".").filter(Boolean);
    const relativeBase = leadingDots > 0 ? baseDir.slice(0, Math.max(0, baseDir.length - (leadingDots - 1))) : [];
    const candidateBase = [...relativeBase, ...moduleParts].join("/");
    const candidates = [`${candidateBase}.py`, `${candidateBase}/__init__.py`, ...suffixModuleCandidates(files, moduleParts, "py")];
    const found = candidates.find((candidate) => files.some((item) => item.path === candidate));
    if (found && found !== file.path) imports.add(found);
  }
  return [...imports];
}

function extractRustImports(file, files) {
  const imports = new Set();
  const baseDir = file.path.split("/").slice(0, -1);
  for (const line of file.lines) {
    const moduleMatch = line.match(/^\s*(?:pub\s+)?mod\s+([A-Za-z_]\w*)\s*;/);
    if (moduleMatch) {
      const name = moduleMatch[1];
      const candidates = [[...baseDir, `${name}.rs`].join("/"), [...baseDir, name, "mod.rs"].join("/")];
      const found = candidates.find((candidate) => files.some((item) => item.path === candidate));
      if (found) imports.add(found);
      continue;
    }
    const useMatch = line.match(/^\s*(?:pub\s+)?use\s+(?:(?:crate|self|super)::)?([A-Za-z_]\w*)/);
    if (!useMatch) continue;
    const found = suffixModuleCandidates(files, [useMatch[1]], "rs").find((candidate) => candidate !== file.path);
    if (found) imports.add(found);
  }
  return [...imports];
}

function extractScriptImports(file, files, extension) {
  const imports = new Set();
  for (const line of file.lines) {
    let specifier = null;
    if (extension === "sh") specifier = line.match(/^\s*(?:source|\.)\s+["']?([^"'\s]+)["']?/)?.[1] ?? null;
    else if (extension === "ps1") {
      specifier = line.match(/^\s*\.\s+["']?([^"'\s]+)["']?/)?.[1] ?? null;
      specifier = specifier?.replace(/^\$PSScriptRoot[\\/]/i, "./") ?? null;
    } else if (extension === "bat" || extension === "cmd") specifier = line.match(/^\s*call\s+["']?([^:"'\s][^"'\s]*\.(?:bat|cmd))["']?/i)?.[1] ?? null;
    if (!specifier) continue;
    const resolved = resolveRelativeFile(file.path, specifier, files);
    if (resolved && resolved !== file.path) imports.add(resolved);
  }
  return [...imports];
}

function extractNativeImports(file, files) {
  const imports = new Set();
  for (const line of file.lines) {
    const specifier = line.match(/^\s*#\s*(?:include|import)\s*"([^"]+)"/)?.[1];
    if (!specifier) continue;
    const resolved = resolveRelativeFile(file.path, specifier, files);
    if (resolved) imports.add(resolved);
  }
  return [...imports];
}

function extractNsisImports(file, files) {
  const imports = new Set();
  for (const line of file.lines) {
    const specifier = line.match(/^\s*!include\s+"([^"]+)"/i)?.[1];
    if (!specifier) continue;
    const resolved = resolveRelativeFile(file.path, specifier, files);
    if (resolved) imports.add(resolved);
  }
  return [...imports];
}

function extractInnoSetupImports(file, files) {
  const imports = new Set();
  for (const line of file.lines) {
    const specifier = line.match(/^\s*#include\s+["']([^"']+)["']/i)?.[1];
    if (!specifier) continue;
    const resolved = resolveRelativeFile(file.path, specifier, files);
    if (resolved) imports.add(resolved);
  }
  return [...imports];
}

export function containsCall(file, body, callName) {
  const extension = file.path.split(".").at(-1)?.toLowerCase();
  const escaped = escapeRegExp(callName);
  if (extension === "sh" || extension === "ps1") return new RegExp(`(?:^|[\\s;&|])${escaped}(?=\\s|$)`, "m").test(body);
  if (extension === "bat" || extension === "cmd") return new RegExp(`\\bcall\\s+:${escaped}\\b`, "i").test(body);
  if (extension === "vbs") return new RegExp(`\\b(?:Call\\s+)?${escaped}\\s*\\(`, "i").test(body);
  if (NSIS_EXTENSIONS.has(extension)) return new RegExp(`\\bCall\\s+${escaped}\\b`, "i").test(body);
  if (INNO_SETUP_EXTENSIONS.has(extension)) return new RegExp(`(?:\\.|\\b)${escaped}\\s*\\(`, "i").test(body);
  return new RegExp(`(?:\\.|\\b)${escaped}\\s*\\(`).test(body);
}

// Inverse of containsCall: instead of testing one candidate name against a body,
// harvest every name the body calls in a single pass. Each pattern below is the
// capturing form of the matching containsCall branch, so for any name N,
// `extractCallNames(file, body).names` contains N exactly when
// `containsCall(file, body, N)` is true. The equivalence is pinned by
// tests/incremental-index.test.mjs.
//
// This is what makes per-file parse results cacheable: call RESOLUTION then needs
// only the global symbol index, never the source text of an unchanged file.
export function extractCallNames(file, body) {
  const extension = file.path.split(".").at(-1)?.toLowerCase();
  const names = new Set();
  const harvest = (pattern) => {
    for (const match of body.matchAll(pattern)) {
      if (match[1]) names.add(match[1]);
    }
  };
  if (extension === "sh" || extension === "ps1") {
    harvest(/(?:^|[\s;&|])([^\s;&|]+)(?=\s|$)/gm);
    return { names: [...names], caseInsensitive: false };
  }
  if (extension === "bat" || extension === "cmd") {
    harvest(/\bcall\s+:(\w+)\b/gi);
    return { names: [...names], caseInsensitive: true };
  }
  if (extension === "vbs") {
    harvest(/\b(?:Call\s+)?(\w+)\s*\(/gi);
    return { names: [...names], caseInsensitive: true };
  }
  if (NSIS_EXTENSIONS.has(extension)) {
    harvest(/\bCall\s+(\w+)\b/gi);
    return { names: [...names], caseInsensitive: true };
  }
  if (INNO_SETUP_EXTENSIONS.has(extension)) {
    harvest(/(?:\.|\b)(\w+)\s*\(/gi);
    return { names: [...names], caseInsensitive: true };
  }
  harvest(/(?:\.|\b)(\w+)\s*\(/g);
  return { names: [...names], caseInsensitive: false };
}

function symbolNode(kind, file, name, qualifiedName, startLine, endLine, labels = ["Symbol"]) {
  return {
    id: `symbol:${file.path}::${qualifiedName}`,
    kind,
    labels,
    name,
    qualifiedName,
    path: file.path,
    evidence: [{ path: file.path, startLine, endLine, contentHash: file.contentHash }],
  };
}

function resolveRelativeFile(sourcePath, specifier, files) {
  const baseDir = sourcePath.split("/").slice(0, -1);
  const raw = [...baseDir, ...normalizePath(specifier).split("/")].filter((part) => part && part !== ".");
  const parts = [];
  for (const part of raw) part === ".." ? parts.pop() : parts.push(part);
  const candidate = parts.join("/");
  return files.some((file) => file.path === candidate) ? candidate : null;
}

function suffixModuleCandidates(files, moduleParts, extension) {
  const suffixes = [`${moduleParts.join("/")}.${extension}`, `${moduleParts.join("/")}/${extension === "py" ? "__init__.py" : "mod.rs"}`];
  return files.map((item) => item.path).filter((candidate) => suffixes.some((suffix) => candidate === suffix || candidate.endsWith(`/${suffix}`))).sort();
}

function findBlockEnd(lines, startIndex) {
  let depth = 0;
  let seenOpen = false;
  for (let index = startIndex; index < lines.length; index += 1) {
    for (const char of lines[index]) {
      if (char === "{") { depth += 1; seenOpen = true; }
      else if (char === "}") { depth -= 1; if (seenOpen && depth <= 0) return index + 1; }
    }
  }
  return startIndex + 1;
}

function findPythonBlockEnd(lines, startIndex) {
  const startIndent = leadingWhitespace(lines[startIndex]);
  let endLine = startIndex + 1;
  for (let index = startIndex + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (!line.trim() || line.trimStart().startsWith("#")) continue;
    if (leadingWhitespace(line) <= startIndent) break;
    endLine = index + 1;
  }
  return endLine;
}

function findBatchBlockEnd(lines, startIndex) {
  for (let index = startIndex + 1; index < lines.length; index += 1) if (/^:[A-Za-z_][\w.-]*\s*$/.test(lines[index])) return index;
  return lines.length;
}

function findVbsBlockEnd(lines, startIndex) {
  for (let index = startIndex + 1; index < lines.length; index += 1) if (/^\s*End\s+(?:Sub|Function)\b/i.test(lines[index])) return index + 1;
  return startIndex + 1;
}

function findPascalRoutineEnd(lines, startIndex) {
  let depth = 0;
  let opened = false;
  for (let index = startIndex + 1; index < lines.length; index += 1) {
    const line = lines[index].replace(/'[^']*'/g, "");
    const begins = line.match(/\bbegin\b/gi)?.length ?? 0;
    const ends = line.match(/\bend\b/gi)?.length ?? 0;
    if (begins > 0) opened = true;
    depth += begins - ends;
    if (opened && depth <= 0) return index + 1;
  }
  return startIndex + 1;
}

function findNamedEnd(lines, startIndex, token) {
  const pattern = new RegExp(`^\\s*${escapeRegExp(token)}\\b`, "i");
  for (let index = startIndex + 1; index < lines.length; index += 1) if (pattern.test(lines[index])) return index + 1;
  return startIndex + 1;
}

function leadingWhitespace(line) {
  return line.match(/^\s*/)?.[0].replaceAll("\t", "    ").length ?? 0;
}

function titleCase(value) {
  return value[0].toUpperCase() + value.slice(1);
}

function normalizePath(value) {
  return String(value).replaceAll("\\", "/").replace(/^\.\//, "");
}

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
