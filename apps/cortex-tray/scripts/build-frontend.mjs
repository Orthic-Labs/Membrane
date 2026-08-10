import { cpSync, mkdirSync, rmSync } from "node:fs";

const output = new URL("../dist/", import.meta.url);
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(new URL("../index.html", import.meta.url), new URL("index.html", output));
cpSync(new URL("../src", import.meta.url), new URL("src", output), { recursive: true });
cpSync(new URL("../node_modules/@tauri-apps/api", import.meta.url), new URL("vendor/@tauri-apps/api", output), { recursive: true });
