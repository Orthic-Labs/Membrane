// Direct coverage for the BM25 lexical discovery lane. Before this file the
// index had no test importing it at all — it was only reached indirectly
// through service-level query tests, so a change to its admission rule could
// silently destroy recall while the corpus stayed green.

import assert from "node:assert/strict";
import test from "node:test";

import { Bm25CodeIndex, buildBm25CodeIndex, tokenizeCodeIdentifiers } from "../src/graph/bm25-code-index.mjs";

function index(names) {
  return new Bm25CodeIndex().replace(names.map((name, i) => ({
    id: `symbol:${name}`,
    name,
    qualifiedName: name,
    path: `src/${name}.js`,
    signature: "",
    identifiers: [],
    node: { id: `symbol:${name}`, name },
  })));
}

const CORPUS = ["UserAccountRepo", "OrderService", "getUserByIdentifier", "placeOrder", "oldValue", "stableValue"];
const hits = (query) => index(CORPUS).search(query, { limit: 10 }).map((row) => row.document.name);

test("a query that over-specifies a stored identifier still finds it", () => {
  // The dominant code-search shape: the caller guesses a longer name than the
  // one in the repository. Abbreviation mismatch, suffix guesses and
  // near-misses must return a ranked near-match, never nothing.
  assert.deepEqual(hits("UserAccountRepository"), ["UserAccountRepo"]);
  assert.deepEqual(hits("getUserById"), ["getUserByIdentifier"]);
  assert.deepEqual(hits("OrderServiceImpl"), ["OrderService"]);
  assert.deepEqual(hits("placeOrderCommand"), ["placeOrder"]);
});

test("a single shared subtoken is not a hit for a two-subtoken identifier", () => {
  // `stableValue` shares only the generic `value` half of `oldValue`. Half is
  // not a majority, so it is not evidence for this query — admitting it made
  // the lane report a symbol the caller never asked about.
  assert.deepEqual(hits("oldValue"), ["oldValue"]);
  assert.deepEqual(hits("stableValue"), ["stableValue"]);
});

test("prose and single-subtoken queries keep plain OR behaviour", () => {
  assert.deepEqual(hits("order").sort(), ["OrderService", "placeOrder"]);
  assert.deepEqual(hits("user account repo").sort(), ["UserAccountRepo", "getUserByIdentifier"]);
});

test("exact name matches outrank partial ones", () => {
  const rows = index([...CORPUS, "order"]).search("order", { limit: 10 });
  assert.equal(rows[0].document.name, "order");
  assert.equal(rows[0].exactName, true);
});

test("tokenizer splits camelCase, snake_case and path separators", () => {
  assert.deepEqual(tokenizeCodeIdentifiers("getUserById"), ["get", "user", "by", "id"]);
  assert.deepEqual(tokenizeCodeIdentifiers("user_account_repo"), ["user", "account", "repo"]);
  assert.deepEqual(tokenizeCodeIdentifiers("src/a.b#c"), ["src"]);
});

test("the index is built only from symbol-like generation nodes", () => {
  const built = buildBm25CodeIndex({ nodes: [
    { id: "f", kind: "file", name: "manifestoDocument", path: "docs/manifestoDocument.md" },
    { id: "s", kind: "symbol", name: "handler", path: "src/a.js" },
  ] });
  assert.deepEqual(built.search("handler", { limit: 5 }).map((row) => row.id), ["s"]);
  // The file node is not a document, so its distinctive name finds nothing.
  // (`path` IS an indexed field on the symbols that are admitted, so a query
  // naming a symbol's file legitimately reaches that symbol.)
  assert.deepEqual(built.search("manifestoDocument", { limit: 5 }).map((row) => row.id), []);
});

test("an empty or operator-only query returns nothing rather than everything", () => {
  assert.deepEqual(hits(""), []);
  assert.deepEqual(hits("   "), []);
});
