import { spawnSync } from "node:child_process";

const candidates = [
  ...(process.env.CORTEX_TEST_PYTHON ? [[process.env.CORTEX_TEST_PYTHON]] : []),
  ...(process.platform === "win32" ? [["py", "-3.11"]] : [["python3"], ["/usr/bin/python3"]]),
];

function supportsJsonschema(command) {
  return spawnSync(command[0], [...command.slice(1), "-c", "import jsonschema"], {
    stdio: "ignore",
  }).status === 0;
}

export const PYTHON = candidates.find(supportsJsonschema);

if (!PYTHON) {
  throw new Error(
    "Cortex tests require Python 3.11+ with jsonschema==4.26.0; install requirements-test.txt or set CORTEX_TEST_PYTHON.",
  );
}
