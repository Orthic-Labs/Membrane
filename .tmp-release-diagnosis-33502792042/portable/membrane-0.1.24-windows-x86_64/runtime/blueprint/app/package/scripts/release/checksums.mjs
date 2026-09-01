#!/usr/bin/env node
// D13: checksums helper — SHA-256 over a file or directory tree.

import { createHash } from "node:crypto";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

export function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function sha256Tree(dir) {
  const hasher = createHash("sha256");
  const walk = (current) => {
    for (const name of readdirSync(current).sort()) {
      const path = join(current, name);
      if (statSync(path).isDirectory()) walk(path);
      else hasher.update(name).update(sha256File(path));
    }
  };
  walk(resolve(dir));
  return hasher.digest("hex");
}
