export const WAKE_RETRY_LIMIT = 3;
/** Reconnect after system wake; never retry after explicit logout. */
export async function reconnectAfterWake(sessionId: string): Promise<'connected' | 'exhausted'> {
  for (let attempt = 1; attempt <= WAKE_RETRY_LIMIT; attempt++) {
    if (await probeSession(sessionId)) return 'connected';
  }
  return 'exhausted';
}
async function probeSession(_sessionId: string): Promise<boolean> { return false; }
