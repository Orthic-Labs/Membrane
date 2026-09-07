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
  assert.deepEqual(tokenizeCodeIdentifiers("src/a.b#c"), ["src", "a", "b", "c"]);
  assert.deepEqual(tokenizeCodeIdentifiers("kebab-case-name"), ["kebab", "case", "name"]);
  assert.deepEqual(tokenizeCodeIdentifiers("pkg.mod:Class#method"), ["pkg", "mod", "class", "method"]);
  // A digit continues the run it sits in; only digit -> Upper is a boundary,
  // which is what Ledger's aliasing does too.
  assert.deepEqual(tokenizeCodeIdentifiers("parseUtf8Bytes"), ["parse", "utf8", "bytes"]);
});

test("tokenizer splits acronym runs from the word that follows", () => {
  // The boundary `/([a-z0-9])([A-Z])/` alone cannot see: the acronym run
  // swallowed the next word whole, so `HTTPServer` indexed as one opaque token
  // and no `server` query could reach it. Ledger's identifier aliasing already
  // splits on upper-run -> Upper+lower; these are the strings the canon names.
  assert.deepEqual(tokenizeCodeIdentifiers("HTTPServer"), ["http", "server"]);
  assert.deepEqual(tokenizeCodeIdentifiers("XMLHttpRequest"), ["xml", "http", "request"]);
  assert.deepEqual(tokenizeCodeIdentifiers("parseJSONResponse"), ["parse", "json", "response"]);
  assert.deepEqual(tokenizeCodeIdentifiers("IOError"), ["io", "error"]);
  // A pure acronym has no following word and must stay whole.
  assert.deepEqual(tokenizeCodeIdentifiers("HTTP"), ["http"]);
  assert.deepEqual(tokenizeCodeIdentifiers("httpServer"), ["http", "server"]);
});

test("an acronym-prefixed symbol is reachable by the word the acronym hid", () => {
  const found = index(["HTTPServer", "XMLHttpRequest", "placeOrder"]);
  assert.ok(found.search("server", { limit: 5 }).map((row) => row.document.name).includes("HTTPServer"));
  assert.ok(found.search("request", { limit: 5 }).map((row) => row.document.name).includes("XMLHttpRequest"));
});

test("non-ASCII identifiers survive tokenization", () => {
  // `[^a-z0-9]` deleted every non-ASCII letter, so these identifiers indexed as
  // nothing at all. Ledger's FTS5 tokenizes them; Blueprint must not diverge.
  assert.deepEqual(tokenizeCodeIdentifiers("cafeteríaService"), ["cafetería", "service"]);
  assert.deepEqual(tokenizeCodeIdentifiers("Ünicode_Wert"), ["ünicode", "wert"]);
  assert.deepEqual(tokenizeCodeIdentifiers("données"), ["données"]);
  // Caseless scripts (\p{Lo}) have no case boundary, exactly as in Ledger's
  // aliasing — the point here is that the CJK text SURVIVES rather than being
  // deleted by an ASCII-only class.
  assert.deepEqual(tokenizeCodeIdentifiers("読み込みHandler"), ["読み込みhandler"]);
  assert.deepEqual(tokenizeCodeIdentifiers("読み込み_Handler"), ["読み込み", "handler"]);
  const found = index(["cafeteríaService", "placeOrder"]);
  assert.deepEqual(found.search("cafetería", { limit: 5 }).map((row) => row.document.name), ["cafeteríaService"]);
});

test("single-character identifiers are indexed and findable", () => {
  // The >= 2 floor silently erased `x`, `i`, `n` — real code. Dropping it must
  // not turn a 1-character query into a corpus-wide containment match.
  assert.deepEqual(tokenizeCodeIdentifiers("x"), ["x"]);
  assert.deepEqual(tokenizeCodeIdentifiers("point_x_y"), ["point", "x", "y"]);
  const found = index(["x", "placeOrder", "maxValue"]);
  assert.deepEqual(found.search("x", { limit: 5 }).map((row) => row.document.name), ["x"]);
  // `maxValue` and `placeOrder` both merely CONTAIN the letter x/a; containment
  // must not admit them.
  assert.ok(!found.search("x", { limit: 5 }).map((row) => row.document.name).includes("maxValue"));
  assert.deepEqual(found.search("a", { limit: 5 }).map((row) => row.document.name), []);

  // The discriminating case for the guard. `maxValue` shares the token `value`
  // with the query, so it scores; the only thing that can keep it out is
  // admission. A 1-character word decided by containment admits it, because
  // "maxvalue" happens to contain the letter x. Deciding it by exact token
  // membership does not — and stays corpus-independent either way.
  const mixed = index(["maxValue", "oldValue", "x"]);
  const names = mixed.search("oldValue x", { limit: 5 }).map((row) => row.document.name);
  assert.ok(names.includes("oldValue"));
  assert.ok(names.includes("x"), "the real 1-character symbol is named by the query");
  assert.ok(!names.includes("maxValue"), "a letter appearing inside an identifier does not name it");
});

test("1-character admission does not depend on what else is indexed", () => {
  const of = (names, query) => index(names).search(query, { limit: 20 }).map((row) => row.document.name);
  const base = ["maxValue", "oldValue", "x"];
  const baseline = of(base, "oldValue x");
  for (const extra of [["xRay", "xAxis"], ["indexValue", "boxValue", "fixValue"]]) {
    assert.deepEqual(of([...base, ...extra], "oldValue x").filter((n) => base.includes(n)), baseline);
  }
});

test("incremental add/replace/remove is equivalent to a cold rebuild", () => {
  // Acceptance evidence for dropping the full-corpus recompute. df, avgdl and
  // search output after a mutation series must be indistinguishable from an
  // index built once from the same final document set.
  const document = (name, extra = "") => ({
    id: `symbol:${name}`,
    name,
    qualifiedName: `mod.${name}`,
    path: `src/${name}.js`,
    signature: extra,
    identifiers: extra ? [extra] : [],
    node: { id: `symbol:${name}`, name },
  });

  const warm = new Bm25CodeIndex().replace(["alpha", "beta", "gamma"].map((n) => document(n)));
  warm.replaceDocument(document("delta"));
  warm.replaceDocument(document("beta", "HTTPServer handler"));
  warm.removeDocument("symbol:alpha");
  warm.replaceDocument(document("epsilon", "XMLHttpRequest"));
  warm.removeDocument("symbol:gamma");
  warm.replaceDocument(document("delta", "renamedPayload"));
  warm.removeDocument("symbol:does-not-exist");

  const final = [
    document("beta", "HTTPServer handler"),
    document("delta", "renamedPayload"),
    document("epsilon", "XMLHttpRequest"),
  ];
  const cold = new Bm25CodeIndex().replace(final);

  assert.deepEqual([...warm.documents.keys()].sort(), [...cold.documents.keys()].sort());
  assert.deepEqual(
    [...warm.df.entries()].sort((a, b) => a[0].localeCompare(b[0])),
    [...cold.df.entries()].sort((a, b) => a[0].localeCompare(b[0])),
    "incremental df must equal a rebuilt df, including having no zero-count entries",
  );
  assert.equal(warm.avgdl, cold.avgdl);
  const strip = (rows) => rows.map(({ id, score, exactName, contributions }) => ({ id, score, exactName, contributions }));
  for (const query of ["beta", "server", "request", "renamedPayload", "delta", "handler", "src"]) {
    assert.deepEqual(strip(warm.search(query, { limit: 20 })), strip(cold.search(query, { limit: 20 })), query);
  }
});

test("removing every document leaves an index equivalent to an empty one", () => {
  const doc = (name) => ({ id: `symbol:${name}`, name, qualifiedName: name, path: `src/${name}.js`, signature: "", identifiers: [], node: {} });
  const warm = new Bm25CodeIndex().replace([doc("alpha"), doc("beta")]);
  warm.removeDocument("symbol:alpha");
  warm.removeDocument("symbol:beta");
  const cold = new Bm25CodeIndex().replace([]);
  assert.deepEqual([...warm.df.entries()], [], "df must be empty, not full of zero counts");
  assert.equal(warm.avgdl, cold.avgdl);
  assert.deepEqual(warm.search("alpha", { limit: 5 }), []);
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
