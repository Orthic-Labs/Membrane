// Push transport delegates to the existing authenticated loopback client.
import { postPlanner, readPort, readToken } from "./client.mjs";
import { MAX_PUSH_REQUEST_BYTES } from "./push-limits.mjs";

export async function pushRequest(operation, body, { env = process.env, signal, request = postPlanner } = {}) {
  const paths = { membrane_push_prepare: "/push/prepare", membrane_push_resolve: "/push/resolve" };
  if (!Object.hasOwn(paths, operation)) throw new Error("push_unknown_operation");
  if (Buffer.byteLength(JSON.stringify(body), "utf8") > MAX_PUSH_REQUEST_BYTES) throw new Error("push_input_limit");
  const timeout = AbortSignal.timeout(2500);
  const response = await request({ host: "127.0.0.1", port: readPort(env), path: paths[operation],
    body, token: readToken(env), traceId: "push", signal: signal ? AbortSignal.any([signal, timeout]) : timeout });
  let parsed;
  try { parsed = JSON.parse(response.body); } catch { throw new Error("push_malformed_response"); }
  if (response.status >= 400 || parsed?.result?.kind !== "success") {
    const code = parsed?.result?.code;
    throw new Error(typeof code === "string" && /^[a-z0-9_]{1,100}$/i.test(code) ? code : "push_unavailable");
  }
  return parsed.result.data;
}
