const DEFAULT_BASE_URL = "http://127.0.0.1:47851";
const DEFAULT_TIMEOUT_MS = 2500;
const MAX_ERROR_DETAIL_BYTES = 480;

export function resolveDiagnosticsBaseUrl(env = process.env) {
  const configured = [env.MEMBRANE_RESIDENT_URL, env.MEMBRANE_LOOPBACK_URL].find((value) => typeof value === "string" && value.trim());
  if (!configured) return DEFAULT_BASE_URL;
  try {
    const url = new URL(configured.trim());
    if (url.protocol !== "http:" && url.protocol !== "https:") return DEFAULT_BASE_URL;
    const path = url.pathname.replace(/\/+$/, "");
    return `${url.protocol}//${url.host}${path}`;
  } catch {
    return DEFAULT_BASE_URL;
  }
}

function parseBody(text) {
  try { return JSON.parse(text); } catch { return text; }
}

function normalizeErrorEnvelope(status, text) {
  let parsed;
  try { parsed = JSON.parse(text); } catch { parsed = null; }
  const error = parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed.error : null;
  if (error && typeof error === "object" && typeof error.code === "string" && error.code.trim()) {
    return {
      code: error.code,
      detail: typeof error.detail === "string" ? error.detail : JSON.stringify(error.detail ?? null),
    };
  }
  return { code: `resident_http_${status}`, detail: text.slice(0, MAX_ERROR_DETAIL_BYTES) };
}

export async function diagnosticsRequest(pathname, options = {}) {
  const { method = "GET", body, timeoutMs = DEFAULT_TIMEOUT_MS, baseUrl, fetchImpl = globalThis.fetch, env = process.env } = options;
  const base = baseUrl ?? resolveDiagnosticsBaseUrl(env);
  const url = `${base}${pathname.startsWith("/") ? pathname : `/${pathname}`}`;
  const headers = { accept: "application/json" };
  if (body !== undefined) headers["content-type"] = "application/json";
  if (typeof env.MEMBRANE_RESIDENT_TOKEN === "string" && env.MEMBRANE_RESIDENT_TOKEN) headers.authorization = `Bearer ${env.MEMBRANE_RESIDENT_TOKEN}`;
  const attempt = () => new Promise((settle) => {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(new Error(`no response within ${timeoutMs}ms`)), Math.max(1, timeoutMs));
    fetchImpl(url, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal: controller.signal }).then(
      async (response) => {
        const text = await response.text();
        clearTimeout(timer);
        settle(response.ok
          ? { ok: true, status: response.status, body: parseBody(text) }
          : { ok: false, status: response.status, body: null, error: normalizeErrorEnvelope(response.status, text) });
      },
      (error) => {
        clearTimeout(timer);
        settle(controller.signal.aborted
          ? { ok: false, status: null, body: null, error: { code: "resident_timeout", detail: `resident did not respond within ${timeoutMs}ms` } }
          : { ok: false, status: null, body: null, error: { code: "resident_unreachable", detail: String(error instanceof Error ? error.message : error) } });
      },
    );
  });
  const first = await attempt();
  if (!first.ok && first.error.code === "resident_unreachable") return attempt();
  return first;
}

export function registerObservedMutation(payload, options = {}) {
  return diagnosticsRequest("/diagnostics/mutation/registerObserved", { method: "POST", body: payload, timeoutMs: 1200, ...options });
}
