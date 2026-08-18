// D45: session tokens for the local explorer — unguessable, held in memory
// only, never logged.

import { randomBytes, timingSafeEqual } from "node:crypto";

export function createSessionToken() {
  return randomBytes(32).toString("base64url");
}

export function verifySessionToken(expected, provided) {
  if (!expected || !provided) return false;
  const a = Buffer.from(expected);
  const b = Buffer.from(String(provided));
  if (a.length !== b.length) return false;
  return timingSafeEqual(a, b);
}
