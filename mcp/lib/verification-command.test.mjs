import assert from "node:assert/strict";
import test from "node:test";
import { isVerificationCommand } from "./verification-command.mjs";

const VERIFICATION_CASES = [
  ["cargo test", true],
  ["cargo check", true],
  ["cargo build", true],
  ["pnpm test", true],
  ["pnpm build", true],
  ["npm test", true],
  ["npm run build", true],
  ["yarn test", true],
  ["yarn build", true],
  ["make", true],
  ["make check", true],
  ["gradle build", true],
  ["mvn test", true],
  ["go test ./...", true],
  ["go build ./...", true],
  ["pnpm run test", true],
  ["npm publish", true],
  ["cargo publish", true],
  ["test", true],
  ["check", true],
  ["build", true],
  ["compile", true],
  ["release", true],
  ["publish", true],
];

const INSPECTION_CASES = [
  ["ls", false],
  ["ls -la", false],
  ["cat src/main.ts", false],
  ["grep pattern src", false],
  ["grep -r test src", false],
  ["rg pattern", false],
  ["rg -n query", false],
  ["git status", false],
  ["git diff", false],
  ["git diff HEAD", false],
  ["git status -s", false],
];

test("verification classifier recognizes intended commands (table-driven)", () => {
  for (const [command, expected] of [...VERIFICATION_CASES, ...INSPECTION_CASES]) {
    assert.equal(isVerificationCommand(command, "Bash"), expected, `command ${JSON.stringify(command)} should be ${expected ? "verification" : "inspection"}`);
  }
});

test("inspection commands remain unfenced even with verification keywords in args", () => {
  assert.equal(isVerificationCommand("grep -r test src/main.ts", "Bash"), false);
  assert.equal(isVerificationCommand("rg build", "Bash"), false);
  assert.equal(isVerificationCommand("git status", "Bash"), false);
  assert.equal(isVerificationCommand("git diff HEAD", "Bash"), false);
});

test("tool-wrapped forms are covered by keyword match", () => {
  assert.equal(isVerificationCommand("cargo test -- --nocapture", "Bash"), true);
  assert.equal(isVerificationCommand("pnpm build", "Bash"), true);
  assert.equal(isVerificationCommand("npm run build", "Bash"), true);
});

test("compound commands classify each top-level segment independently", () => {
  const cases = [
    ["ls; pnpm test", true],
    ["git status && rightkit cargo test", true],
    ["rg foo || npm run build", true],
    ["cat file | grep text", false],
    ["cat file | cargo check", true],
    ["printf 'pnpm test'", true],
    ["grep 'pnpm test' src", false],
  ];
  for (const [command, expected] of cases) assert.equal(isVerificationCommand(command, "Bash"), expected, command);
});

test("unbalanced quoting fails closed when verification is present", () => {
  assert.equal(isVerificationCommand("grep 'test", "Bash"), true);
  assert.equal(isVerificationCommand("ls 'src", "Bash"), false);
});
