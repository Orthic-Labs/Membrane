// D05: CLI arg parsing and stable exit codes (S-04 contract).

export const EXIT = Object.freeze({
  OK: 0,
  USAGE: 1,
  DEGRADED: 2,
  STALE_BLOCKED: 3,
  CONTRACT: 4,
  SECURITY: 5,
  INTERNAL: 10,
});

export function parseArgs(argv = []) {
  const args = { _: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--") {
      args._.push(...argv.slice(index + 1));
      break;
    }
    if (token.startsWith("--")) {
      const [name, ...valueParts] = token.slice(2).split("=");
      if (valueParts.length) {
        args[name] = valueParts.join("=");
        continue;
      }
      const next = argv[index + 1];
      if (next !== undefined && !next.startsWith("-")) {
        args[name] = next;
        index += 1;
      } else {
        args[name] = true;
      }
      continue;
    }
    if (token.startsWith("-") && token.length > 1) {
      const name = token.slice(1);
      args[name] = true;
      continue;
    }
    args._.push(token);
  }
  return args;
}

export function jsonFlag(args) {
  return Boolean(args.json);
}

export function commandName(argv) {
  const [command, ...rest] = argv;
  if (!command) return { command: null, rest };
  return { command, rest };
}
