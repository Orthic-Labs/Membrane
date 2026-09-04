// This is an owned result boundary, not a claim to intercept unrelated tools.
// Call only after execution. No command, retries or provider ranking live here.
import { pushRequest } from "../push-client.mjs";
import { createHash } from "node:crypto";
const size = (value) => Buffer.byteLength(JSON.stringify(value), "utf8");
const digest = (text) => `sha256:${createHash("sha256").update(text).digest("hex")}`;

export async function prepareToolEgress(result, binding, options = {}) {
  const { resolverToken, maxBytes, kind = "text", env = process.env, signal, request = pushRequest } = options;
  const kept = (reason) => ({ result, state: Number.isSafeInteger(maxBytes) && maxBytes >= 2048 && size(result) > maxBytes ? "refused" : "passthrough", receipt: { reason, observed: true, savingsBytes: 0 } });
  if (!result || result.isError || result.disposition === "exact" || result.structuredContent?.data?.disposition === "exact" || result.structuredContent?.result?.data?.disposition === "exact" || result.structuredContent?.data?.pushRepresentation) return kept("exact_or_error");
  if (!Array.isArray(result.content) || result.content.length !== 1 || result.content[0]?.type !== "text" || typeof result.content[0].text !== "string") return kept("unsupported_parts");
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 2048 || maxBytes > 49152) return kept("invalid_budget");
  if (typeof resolverToken !== "string" || !/^[a-f0-9]{64}$/.test(resolverToken)) return kept("resolver_unavailable");
  const originalBytes = size(result);
  if (originalBytes <= maxBytes) return kept("within_budget");
  let innerBudget = Math.max(1, maxBytes - 1024);
  for (let attempt = 0; attempt < 3; attempt++) {
    let delivery;
    try {
      delivery = await request("membrane_push_prepare", { ...binding, request: {
        text: result.content[0].text, kind, maxBytes: innerBudget, resolverToken,
        exact: false, optimize: true,
      } }, { env, signal });
    } catch (error) { return kept(signal?.aborted ? "cancelled" : "prepare_refused"); }
    if (delivery?.receipt?.sourceDigest !== digest(result.content[0].text) || delivery?.receipt?.representationDigest !== digest(delivery?.text ?? "")) return kept("delivery_identity_mismatch");
    if (delivery.disposition !== "prepared" || !delivery.recovery?.handle) return kept("not_reduced");
    const marker = `\n[Push original: ${delivery.recovery.handle}; expiresAt=${delivery.recovery.expiresAt}; resolve with membrane_push_resolve]`;
    const candidate = { ...result, content: [{ ...result.content[0], text: delivery.text + marker }] };
    // Opt-in consumers receive one model-facing body, never an original echoed
    // through structuredContent. Keep the established data/trace output shape.
    if (result.structuredContent) candidate.structuredContent = { ...result.structuredContent,
      data: { pushRepresentation: delivery.representationKind, recovery: delivery.recovery, receipt: delivery.receipt } };
    const measured = size(candidate);
    if (measured <= maxBytes && measured < originalBytes) return { result: candidate, state: "prepared",
      receipt: { ...delivery.receipt, envelopeBytes: measured, envelopeBasis: "utf8_mcp_tool_result_v1", savingsBytes: originalBytes - measured } };
    innerBudget = Math.max(1, innerBudget - Math.max(256, measured - maxBytes));
  }
  return kept("final_envelope_does_not_fit");
}
