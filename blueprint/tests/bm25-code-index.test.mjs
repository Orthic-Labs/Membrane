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

const CORPUS = ["UserAccountRepo", "UserRepo", "OrderService", "getUserByIdentifier", "placeOrder", "oldValue", "stableValue", "getOldValue", "getStableValue"];
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

test("a document that does not name the query is never a hit, however many subtokens it shares", () => {
  // `stableValue` shares only the generic `value` half of `oldValue`, and
  // neither identifier names the other. These cases add a shared prefix, a
  // path segment and an extension — each defeated an earlier coverage-fraction
  // rule, and none of them makes either identifier contain the other.
  for (const query of ["oldValue", "getOldValue", "src/oldValue", "oldValue.js"]) {
    const found = hits(query);
    assert.ok(found.includes("oldValue"), `${query} must still find oldValue`);
    assert.ok(!found.includes("stableValue"), `${query} must not report stableValue`);
    assert.ok(!found.includes("getStableValue"), `${query} must not report getStableValue`);
  }
});

test("an over-specified query still finds the shorter identifier at two subtokens", () => {
  // The class of query a majority rule silently killed: the unmatched half is
  // a token the corpus has never seen, so it discriminates nothing and must
  // not veto the match.
  assert.ok(hits("UserRepository").includes("UserRepo"));
  assert.ok(hits("OrderServices").includes("OrderService"));
  assert.ok(hits("oldValues").includes("oldValue"));
});

test("a query naming a symbol that does not exist returns nothing, not its generic half", () => {
  // The half-unknown query is the common real case: a renamed or misremembered
  // symbol. A rule that drops the unknown half and answers on the generic one
  // reports whatever happens to share it.
  assert.deepEqual(hits("ZzzQqqWidget"), []);
  for (const query of ["frobnicatedValue", "brandNewValue", "obsoleteValue"]) {
    assert.deepEqual(hits(query), [], `${query} names nothing in the corpus`);
  }
});

test("admission does not depend on what else is indexed", () => {
  // The decisive property. An earlier rule keyed on document frequency, so
  // indexing two unrelated `old*` symbols re-admitted `stableValue` and a
  // third inverted the ordering entirely. Admission must read only the query
  // and the document being judged.
  const of = (names, query) => index(names).search(query, { limit: 20 }).map((row) => row.document.name);
  const baseline = of(CORPUS, "oldValue");
  assert.ok(baseline.includes("oldValue") && !baseline.includes("stableValue"));
  for (const extra of [["oldHandler", "oldParser"], ["oldHandler", "oldParser", "oldCache"], ["valueOne", "valueTwo", "valueThree"]]) {
    const grown = of([...CORPUS, ...extra], "oldValue");
    assert.ok(grown.includes("oldValue"), `oldValue must still match after indexing ${extra.join(", ")}`);
    assert.ok(!grown.includes("stableValue"), `stableValue must stay excluded after indexing ${extra.join(", ")}`);
    assert.ok(!grown.includes("getStableValue"), `getStableValue must stay excluded after indexing ${extra.join(", ")}`);
  }
});

test("a shared path segment alone does not qualify a document", () => {
  // `path` is an indexed field, so every document in the corpus shares `src`.
  // A word that also carries a rarer subtoken must be decided by the rare one.
  assert.ok(!hits("src/oldValue").includes("placeOrder"));
  // A query that is ONLY a directory name legitimately reaches everything
  // under it — that is a directory query, not a symbol query.
  assert.equal(hits("src").length, CORPUS.length);
});

test("prose and single-subtoken queries keep plain OR behaviour", () => {
  assert.deepEqual(hits("order").sort(), ["OrderService", "placeOrder"]);
  // Each word is a single subtoken, so each is its own rarest token and the
  // union is returned — plain OR, unchanged.
  assert.deepEqual(hits("user account repo").sort(), ["UserAccountRepo", "UserRepo", "getUserByIdentifier"]);
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
