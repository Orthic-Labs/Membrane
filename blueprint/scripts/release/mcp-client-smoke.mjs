import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

export async function verifyMcpInitialize({ script, root, cwd = root, timeoutMs = 10000 } = {}) {
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [script, "--root", root],
    cwd,
    stderr: "pipe",
  });
  let stderr = "";
  transport.stderr?.on("data", (chunk) => { stderr += chunk.toString(); });
  const client = new Client({ name: "cortex-smoke", version: "1.0.0" }, { capabilities: {} });
  let timer;
  try {
    const result = await Promise.race([
      (async () => {
        await client.connect(transport);
        const tools = await client.listTools();
        return { serverInfo: client.getServerVersion(), toolCount: tools.tools.length };
      })(),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`MCP initialize timed out after ${timeoutMs}ms`)), timeoutMs);
      }),
    ]);
    if (!result.serverInfo?.name || result.toolCount !== 6) {
      throw new Error(`MCP initialize returned an invalid surface: ${JSON.stringify(result)}`);
    }
    return result;
  } catch (error) {
    throw new Error(`${error.message}${stderr ? `\n${stderr.trim()}` : ""}`);
  } finally {
    clearTimeout(timer);
    await client.close().catch(() => {});
  }
}
