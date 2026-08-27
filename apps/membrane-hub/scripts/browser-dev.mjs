import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const port = Number(process.env.MEMBRANE_HUB_DEV_PORT || 1420);
const fixture = JSON.parse(await readFile(new URL("../tests/fixtures/hub-snapshot-running.json", import.meta.url), "utf8"));

fixture.observedAtUnixMs = Date.now();
fixture.admission = {
  schemaVersion: 1,
  windowHours: 24,
  decisionsTotal: 12,
  omissionsTotal: 3,
  omissionsByReason: [
    { reason: "cross_root", count: 1 },
    { reason: "budget_exhausted", count: 2 },
  ],
  budgetPressureTotal: 2,
  budgetPressureByReason: [{ reason: "budget_exhausted", count: 2 }],
};

const runtime = { serviceState: "running", snapshotState: "available", lastReason: "development_fixture" };
const injection = `<script type="module">
let fixtureAttempts = 0;
const showFixture = () => {
  if (!window.__membraneHub) {
    if (fixtureAttempts++ < 250) setTimeout(showFixture, 20);
    return;
  }
  window.__membraneHub.render(${JSON.stringify(fixture)}, ${JSON.stringify(runtime)});
  if (new URLSearchParams(location.search).has("focus")) document.querySelector('a[aria-current="page"]')?.focus();
  const crumb = document.querySelector("#crumb");
  if (crumb) crumb.textContent = "DEVELOPMENT FIXTURE · hub.snapshot";
  const badge = document.createElement("div");
  badge.textContent = "DEVELOPMENT FIXTURE";
  badge.setAttribute("aria-label", "Dashboard is displaying deterministic development fixture data");
  Object.assign(badge.style, { position: "fixed", right: "12px", bottom: "10px", zIndex: "9999", padding: "5px 8px", border: "1px solid #7c4dff", borderRadius: "5px", background: "#18151f", color: "#bb8cff", font: "600 10px/1.2 Inter, sans-serif", letterSpacing: ".08em" });
  document.body.append(badge);
};
showFixture();
</script>`;

const mime = new Map([
  [".html", "text/html; charset=utf-8"], [".mjs", "text/javascript; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"], [".css", "text/css; charset=utf-8"],
  [".json", "application/json; charset=utf-8"], [".svg", "image/svg+xml"],
  [".woff2", "font/woff2"], [".png", "image/png"],
]);

createServer(async (request, response) => {
  try {
    const pathname = decodeURIComponent(new URL(request.url, `http://${request.headers.host}`).pathname);
    const requested = pathname === "/" ? "index.html" : pathname.replace(/^\/+/, "");
    const file = resolve(root, requested);
    if (file !== root && !file.startsWith(`${root}${sep}`)) throw new Error("outside root");
    let body = await readFile(file);
    if (extname(file) === ".html") body = Buffer.from(body.toString("utf8").replace("</body>", `${injection}</body>`));
    response.writeHead(200, { "content-type": mime.get(extname(file)) || "application/octet-stream", "cache-control": "no-store" });
    response.end(body);
  } catch {
    response.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    response.end("Not found");
  }
}).listen(port, "127.0.0.1", () => {
  process.stdout.write(`Membrane Hub browser dev: http://127.0.0.1:${port}/\n`);
});
