import { contentDigest } from "./generation-identity.mjs";

function normalizeDigest(value) {
  const text = String(value ?? "");
  return text.startsWith("xxh128:") ? text : `xxh128:${text}`;
}

export function leafDigestForFile(contentDigestValue) {
  return normalizeDigest(contentDigestValue);
}

export function dirDigest(childEntries) {
  const entries = [...(childEntries ?? [])]
    .map((entry) => [String(entry.name), String(entry.digest)])
    .sort(([left], [right]) => left.localeCompare(right));
  return contentDigest(Buffer.from(JSON.stringify(entries), "utf8"));
}

function parentDirectories(path) {
  const parts = String(path).replaceAll("\\", "/").split("/").filter(Boolean);
  const parents = [""];
  for (let index = 1; index < parts.length; index += 1) parents.push(parts.slice(0, index).join("/"));
  return parents;
}

function pathMetadata(path) {
  const normalized = String(path).replaceAll("\\", "/");
  if (normalized === "") return { parentPath: null, name: "" };
  const split = normalized.lastIndexOf("/");
  return { parentPath: split >= 0 ? normalized.slice(0, split) : "", name: split >= 0 ? normalized.slice(split + 1) : normalized };
}

function writeDirectory(db, directory) {
  const children = db.prepare("SELECT name, digest FROM generation_leaf WHERE parent_path = ? ORDER BY name").all(directory);
  if (directory !== "" && children.length === 0) {
    db.prepare("DELETE FROM generation_leaf WHERE path = ? AND kind = 'dir'").run(directory);
    return null;
  }
  const digest = dirDigest(children);
  const { parentPath, name } = pathMetadata(directory);
  db.prepare(`INSERT INTO generation_leaf(path, kind, digest, parent_path, name) VALUES (?, 'dir', ?, ?, ?)
    ON CONFLICT(path) DO UPDATE SET kind='dir', digest=excluded.digest, parent_path=excluded.parent_path, name=excluded.name`)
    .run(directory, digest, parentPath, name);
  return digest;
}

function rebuildDirectories(db) {
  db.prepare("DELETE FROM generation_leaf WHERE kind = 'dir'").run();
  const directories = new Set([""]);
  for (const row of db.prepare("SELECT path FROM generation_leaf WHERE kind = 'file'").all()) {
    for (const parent of parentDirectories(row.path)) directories.add(parent);
  }
  for (const directory of [...directories].sort((left, right) => right.split("/").length - left.split("/").length || right.localeCompare(left))) {
    writeDirectory(db, directory);
  }
  return db.prepare("SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'").get()?.digest ?? dirDigest([]);
}

export function updateLeafChain(db, path, contentDigestOrNull) {
  const normalized = String(path).replaceAll("\\", "/");
  if (contentDigestOrNull === null || contentDigestOrNull === undefined) {
    db.prepare("DELETE FROM generation_leaf WHERE path = ? AND kind = 'file'").run(normalized);
  } else {
    const { parentPath, name } = pathMetadata(normalized);
    db.prepare(`INSERT INTO generation_leaf(path, kind, digest, parent_path, name) VALUES (?, 'file', ?, ?, ?)
      ON CONFLICT(path) DO UPDATE SET kind='file', digest=excluded.digest, parent_path=excluded.parent_path, name=excluded.name`)
      .run(normalized, leafDigestForFile(contentDigestOrNull), parentPath, name);
  }
  const parents = parentDirectories(normalized).sort((left, right) => right.split("/").length - left.split("/").length || right.localeCompare(left));
  for (const directory of parents) writeDirectory(db, directory);
  return db.prepare("SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'").get()?.digest ?? dirDigest([]);
}

export function computeFullLedger(db, files) {
  db.exec("DELETE FROM generation_leaf");
  const insert = db.prepare("INSERT INTO generation_leaf(path, kind, digest, parent_path, name) VALUES (?, 'file', ?, ?, ?)");
  for (const file of files ?? []) {
    const digest = file.contentDigest ?? file.content_digest ?? file.contentHash;
    if (digest) {
      const path = String(file.path).replaceAll("\\", "/");
      const { parentPath, name } = pathMetadata(path);
      insert.run(path, leafDigestForFile(digest), parentPath, name);
    }
  }
  return rebuildDirectories(db);
}

export function diffLedgerAgainstTree(db, root, scanFiles) {
  const recorded = new Map(db.prepare("SELECT path, digest FROM generation_leaf WHERE kind = 'file'").all().map((row) => [row.path, row.digest]));
  const current = new Map((scanFiles ?? []).map((file) => [String(file.path), leafDigestForFile(file.contentDigest ?? file.content_digest ?? file.contentHash)]));
  const changed = [];
  const added = [];
  const removed = [];
  for (const [path, digest] of current) {
    if (!recorded.has(path)) added.push(path);
    else if (recorded.get(path) !== digest) changed.push(path);
  }
  for (const path of recorded.keys()) if (!current.has(path)) removed.push(path);
  return {
    changed: changed.sort(),
    added: added.sort(),
    removed: removed.sort(),
    root: root ?? db.prepare("SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'").get()?.digest ?? null,
  };
}
