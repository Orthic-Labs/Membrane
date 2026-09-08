// Isolated diagnostic: does the SHIPPED tree-sitter provider change retrieval
// outcomes vs the lexical-only graph the checked-in benchmark measures?
// Same corpus, same seeds/expansion/scoring rule, only the provider differs.
import { readFileSync, existsSync, mkdtempSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildGraphGeneration, queryGraph, augmentGenerationWithTreeSitter } from "../../../../blueprint/src/graph/static-provider.mjs";

const BP = new URL("../../../../blueprint/", import.meta.url).pathname;
const corpus = JSON.parse(readFileSync(join(BP, "evals/retrieval-corpus/corpus.v1.json"), "utf8"));
const FIX = join(BP, "evals/fixture-repos");

function terms(query) {
  const stop = new Set(["a","an","and","for","in","of","the","to","where"]);
  return [...new Set(String(query).replace(/([a-z0-9])([A-Z])/g,"$1 $2").toLowerCase()
    .split(/[^a-z0-9_]+/).filter(t=>t.length>1 && !stop.has(t)))];
}
function nodePath(n){ return n.path ?? n.evidence?.[0]?.path ?? null; }
function expandHybrid(gen, query, limit) {
  const seeds = new Set(queryGraph(gen, { query, limit: 5 }).map(n=>n.id));
  if (!seeds.size) return [];
  const byId = new Map(gen.nodes.map(n=>[n.id,n]));
  const depth = new Map([...seeds].map(id=>[id,0]));
  for (const e of (gen.edges||[])) {
    const a = e.from ?? e.source, b = e.to ?? e.target;
    if (seeds.has(a) && !depth.has(b)) depth.set(b,1);
    if (seeds.has(b) && !depth.has(a)) depth.set(a,1);
  }
  const t = terms(query);
  return [...depth.keys()].map(id=>byId.get(id)).filter(Boolean).map(n=>{
    const hay = `${n.kind??""} ${n.name??""} ${n.qualifiedName??""} ${nodePath(n)??""}`.toLowerCase();
    const s = t.reduce((sum,x)=>sum+(hay.includes(x)?1:0),0);
    const d = depth.get(n.id);
    return { id:n.id, kind:n.kind, name:n.name, qualifiedName:n.qualifiedName, path:nodePath(n),
             evidence:n.evidence, score: s*(d===0?1:0.5), isSeed:d===0 };
  }).filter(n=>n.score>0 || seeds.has(n.id))
    .sort((a,b)=>b.score-a.score || (Number(b.isSeed)-Number(a.isSeed)) || a.id.localeCompare(b.id))
    .slice(0,limit);
}
function score(caseEntry, list, fileText) {
  const gold = caseEntry.gold;
  if (gold.abstained) return { hit: list.length===0, reason: list.length===0?"abstained_correctly":"junk_on_no_gold", rank: list.length===0?1:Infinity };
  if (list.length===0) return { hit:false, reason:"abstained_on_positive_gold", rank:Infinity };
  for (let i=0;i<list.length;i++){
    const p = String(list[i].path ?? "");
    if (!(p===gold.path || p.endsWith("/"+gold.path))) continue;
    if (Array.isArray(gold.region) && gold.region.length===2) {
      const lines=(fileText||"").split(/\r?\n/);
      const slice=lines.slice(Math.max(0,gold.region[0]-1), Math.min(lines.length,gold.region[1])).join("\n");
      if (!(gold.region[0]===1 && gold.region[1]>=200) && slice.length===0) continue;
      if (gold.region[0]===1 && gold.region[1]>=200 && (fileText||"").length===0) continue;
    }
    return { hit:true, reason:"hit", rank:i+1 };
  }
  return { hit:false, reason:"miss", rank:Infinity };
}

const repos = new Map();
for (const r of new Set(corpus.cases.map(c=>c.repo))) {
  const work = mkdtempSync(join(tmpdir(), `bp-ts-${r}-`));
  cpSync(join(FIX,r), work, { recursive: true });
  const lex = buildGraphGeneration(work, {});
  const ts  = buildGraphGeneration(work, {});
  const summary = await augmentGenerationWithTreeSitter(ts, work, {});
  repos.set(r, { work, lex, ts, tsState: summary?.state ?? "ok", provider: ts.manifest?.provider?.id, parsed: summary?.summary?.parsedFiles });
}
for (const [r,v] of repos) console.log(`repo=${r} lexNodes=${v.lex.nodes.length} tsNodes=${v.ts.nodes.length} provider=${v.provider} tsState=${v.tsState}`);

const arms = { lexicalOnlyGraph: "lex", treesitterGraph: "ts" };
const agg = {};
for (const name of Object.keys(arms)) agg[name] = { hits:0, total:0, mrr:0, abstainedCorrectly:0, junkOnNoGold:0, abstainedOnPositiveGold:0 };
const perCase = [];
for (const c of corpus.cases) {
  const rec = repos.get(c.repo);
  const full = c.gold?.path ? join(rec.work, c.gold.path) : null;
  const text = full && existsSync(full) ? readFileSync(full,"utf8") : "";
  const row = { id: c.id, class: c.class };
  for (const [name,key] of Object.entries(arms)) {
    const list = expandHybrid(rec[key], c.query, 10);
    const s = score(c, list, text);
    row[name] = s.reason;
    agg[name].total++; if (s.hit) agg[name].hits++;
    agg[name].mrr += 1/s.rank;
    if (s.reason==="abstained_correctly") agg[name].abstainedCorrectly++;
    if (s.reason==="junk_on_no_gold") agg[name].junkOnNoGold++;
    if (s.reason==="abstained_on_positive_gold") agg[name].abstainedOnPositiveGold++;
  }
  perCase.push(row);
}
for (const [n,a] of Object.entries(agg))
  console.log(`${n.padEnd(20)} hitRate=${(a.hits/a.total*100).toFixed(1)}% mrr=${(a.mrr/a.total).toFixed(3)} abstainCorrect=${a.abstainedCorrectly} junkNoGold=${a.junkOnNoGold} abstainOnPos=${a.abstainedOnPositiveGold} n=${a.total}`);
console.log("\ndifferences:");
for (const r of perCase) if (r.lexicalOnlyGraph !== r.treesitterGraph) console.log(" ", r.id, r.class, r.lexicalOnlyGraph, "->", r.treesitterGraph);
