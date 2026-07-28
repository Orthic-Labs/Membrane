import assert from "node:assert/strict";
import { createNativeInstaller } from "./install.mjs";

const nodePath = "C:\\node\\node.exe";
const serverPath = "D:\\Claude\\membrane\\mcp\\server.mjs";
const expected = () => JSON.stringify({ command: nodePath, args: [serverPath] });

function fakeRunner(state, { missing = [], failAdd = [] } = {}) {
  const calls = [];
  const runner = async (binary, args, options) => {
    calls.push({ binary, args, options });
    const client = binary;
    if (args[0] === "--version") return missing.includes(client) ? { code: 127, stdout: "", stderr: "missing" } : { code: 0, stdout: "v1", stderr: "" };
    if (args[1] === "get") {
      const current = state[client];
      return current ? { code: 0, stdout: current, stderr: "" } : { code: 1, stdout: "", stderr: "not found" };
    }
    if (args[1] === "remove") { state[client] = undefined; return { code: 0, stdout: "", stderr: "" }; }
    if (args[1] === "add") {
      if (failAdd.includes(client)) return { code: 1, stdout: "", stderr: "add failed" };
      const marker = args.indexOf("--");
      state[client] = JSON.stringify({ command: args[marker + 1], args: args.slice(marker + 2) });
      return { code: 0, stdout: "", stderr: "" };
    }
    throw new Error(`unexpected command ${binary} ${args.join(" ")}`);
  };
  return { runner, calls };
}

{
  const state = { codex: undefined };
  const fake = fakeRunner(state);
  const installer = createNativeInstaller({ runner: fake.runner, nodePath, serverPath });
  const result = await installer.install("D:\\repo", ["codex"], { dryRun: true });
  assert.equal(result.dry_run, true);
  assert.equal(state.codex, undefined);
  assert.equal(fake.calls.filter((call) => call.args.includes("add")).length, 0);
  assert.ok(result.commands[0].includes(nodePath));
  assert.ok(fake.calls.every((call) => call.options.windowsHide === true));
}

{
  const state = { codex: undefined };
  const fake = fakeRunner(state);
  const installer = createNativeInstaller({ runner: fake.runner, nodePath, serverPath });
  const receipt = await installer.install("D:\\repo", ["codex"]);
  assert.equal(receipt.clients[0].before, "absent");
  assert.equal(receipt.clients[0].after, "installed");
  assert.equal(state.codex, expected());
  const noOp = await installer.install("D:\\repo", ["codex"]);
  assert.equal(noOp.clients[0].before, "already_correct", state.codex);
  assert.equal(noOp.clients[0].after, "unchanged");
  assert.equal(fake.calls.filter((call) => call.args.includes("add")).length, 1);
}

{
  const prior = JSON.stringify({ command: "C:\\old\\node.exe", args: ["C:\\old\\server.mjs"] });
  const state = { claude: prior };
  const fake = fakeRunner(state);
  const installer = createNativeInstaller({ runner: fake.runner, nodePath, serverPath });
  const receipt = await installer.install("D:\\repo", ["claude"], { claudeScope: "project" });
  assert.equal(receipt.clients[0].before, "conflict");
  assert.equal(state.claude, expected());
  await installer.uninstall("D:\\repo", receipt.clients, { claudeScope: "project" });
  assert.equal(state.claude, prior);
}

{
  const state = { codex: undefined, claude: undefined };
  const fake = fakeRunner(state, { failAdd: ["claude"] });
  const installer = createNativeInstaller({ runner: fake.runner, nodePath, serverPath });
  await assert.rejects(() => installer.install("D:\\repo", ["codex", "claude"]), /claude add failed/);
  assert.equal(state.codex, undefined);
  assert.equal(state.claude, undefined);
}

{
  const fake = fakeRunner({ codex: undefined }, { missing: ["codex"] });
  const installer = createNativeInstaller({ runner: fake.runner, nodePath, serverPath });
  await assert.rejects(() => installer.install("D:\\repo", ["codex"]), /not installed/);
}

process.stdout.write("install mocked lifecycle tests passed\n");
