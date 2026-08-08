import { cpSync, mkdirSync, rmSync } from "node:fs";

const output = new URL("../dist/", import.meta.url);
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
for (const name of ["index.html", "popover.html", "src"]) {
  cpSync(new URL(`../${name}`, import.meta.url), new URL(name, output), { recursive: true });
}
cpSync(
  new URL("../node_modules/@tauri-apps/api", import.meta.url),
  new URL("vendor/@tauri-apps/api", output),
  { recursive: true },
);
