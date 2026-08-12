import { cpSync, mkdirSync, rmSync, writeFileSync, readFileSync } from "node:fs";

const output = new URL("../dist/", import.meta.url);
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
for (const name of ["index.html", "popover.html", "src"]) {
  cpSync(new URL(`../${name}`, import.meta.url), new URL(name, output), { recursive: true });
}
cpSync(new URL("../assets/tray", import.meta.url), new URL("assets/tray", output), { recursive: true });
cpSync(
  new URL("../node_modules/@tauri-apps/api", import.meta.url),
  new URL("vendor/@tauri-apps/api", output),
  { recursive: true, dereference: true },
);
cpSync(
  new URL("../node_modules/@tauri-apps/plugin-os/dist-js", import.meta.url),
  new URL("vendor/@tauri-apps/plugin-os", output),
  { recursive: true, dereference: true },
);
cpSync(
  new URL("../node_modules/@rightkit/platform-ui/dist", import.meta.url),
  new URL("vendor/@rightkit/platform-ui", output),
  { recursive: true, dereference: true },
);

let identity;
try {
  const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  identity = {
    commit: process.env.ORTHIC_SOURCE_COMMIT || "0000000000000000000000000000000000000000",
    dirty: false,
    fileCount: 0,
    sourceTreePath: "orthic",
    sourceTreeSha256: process.env.ORTHIC_SOURCE_TREE_SHA256 || "0".repeat(64),
    releaseGeneration: `sha256:${process.env.ORTHIC_SOURCE_TREE_SHA256 || "0".repeat(64)}`,
    version: pkg.version,
  };
} catch {
  identity = {
    commit: "0000000000000000000000000000000000000000",
    dirty: false,
    fileCount: 0,
    sourceTreePath: "orthic",
    sourceTreeSha256: "0".repeat(64),
    releaseGeneration: `sha256:${"0".repeat(64)}`,
  };
}
writeFileSync(new URL("../dist/release-identity.json", import.meta.url), `${JSON.stringify(identity, null, 2)}\n`);
