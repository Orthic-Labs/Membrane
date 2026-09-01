// D45: loopback-only read-only HTTP server (S-25). Binds to 127.0.0.1, uses
// an unguessable session token, and serves only application-service
// endpoints. Browser assets receive no filesystem path, SQLite handle, host
// credentials, unrestricted query endpoint, or network-capable API.

import http from "node:http";
import { randomBytes } from "node:crypto";
import { URL } from "node:url";
import { verifySessionToken } from "./session-token.mjs";

function json(response, status, value) {
  const body = JSON.stringify(value);
  response.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "content-length": Buffer.byteLength(body),
    "cache-control": "no-store",
    "content-security-policy": "default-src 'none'; frame-ancestors 'none'",
    "x-content-type-options": "nosniff",
  });
  response.end(body);
}

export async function startExplorerServer({ service, repoId, repoRoot, serveAsset }) {
  const sessionToken = randomBytes(32).toString("base64url");
  const routes = new Map([
    ["/api/status", () => service.status({ repoId, repoRoot })],
    ["/api/search", (url) => service.search({ repoId, repoRoot, query: url.searchParams.get("q") ?? "", limit: Number(url.searchParams.get("limit") ?? 20) })],
    ["/api/impact", (url) => service.impact({ repoId, repoRoot, anchor: url.searchParams.get("anchor") ?? "", depth: Number(url.searchParams.get("depth") ?? 3), budget: Number(url.searchParams.get("budget") ?? 2000) })],
    ["/api/architecture", (url) => service.architecture({ repoId, repoRoot, budget: Number(url.searchParams.get("budget") ?? 2000) })],
    ["/api/doc-truth", (url) => service.documentTruth({ repoId, repoRoot, claimId: url.searchParams.get("claimId") ?? undefined, limit: Number(url.searchParams.get("limit") ?? 200) })],
  ]);

  const server = http.createServer(async (request, response) => {
    try {
      if (request.method !== "GET") return json(response, 405, { error: { code: "method_not_allowed" } });
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      const route = routes.get(url.pathname);
      if (url.pathname.startsWith("/api/") && !verifySessionToken(sessionToken, request.headers.authorization?.replace(/^Bearer\s+/, ""))) {
        return json(response, 401, { error: { code: "unauthorized" } });
      }
      if (route) return json(response, 200, await route(url));
      if (url.pathname.startsWith("/api/")) return json(response, 404, { error: { code: "route_not_found" } });
      return serveAsset(request, response, { sessionToken });
    } catch (error) {
      return json(response, Number(error.statusCode ?? 500), {
        schemaVersion: 1,
        error: { code: error.code ?? "internal_error", message: String(error.message ?? error) },
      });
    }
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  return Object.freeze({
    url: `http://127.0.0.1:${address.port}/#token=${sessionToken}`,
    close: () => new Promise((resolve, reject) => {
      let settled = false;
      const finish = (error) => {
        if (settled) return;
        settled = true;
        clearTimeout(guard);
        if (error) reject(error); else resolve();
      };
      const guard = setTimeout(() => finish(), 1000);
      server.close(finish);
      server.closeIdleConnections?.();
      server.closeAllConnections?.();
    }),
  });
}
