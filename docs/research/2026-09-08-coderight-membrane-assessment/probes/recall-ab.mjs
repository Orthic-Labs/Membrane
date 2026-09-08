// A/B: current production seed resolver vs one-line variant that also tries the
// whole (unsplit) query token in the exact-match lanes. Same graph, same corpus,
// same scoring. Ground truth derived from source text.
import { readFileSync, readdirSync, statSync, mkdtempSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { buildGraphGeneration } from "../../../../blueprint/src/graph/static-provider.mjs";
import { openStoreReadOnly } from "../../../../blueprint/src/graph/store-sqlite.mjs";
import * as base from "../../../../blueprint/src/graph/recall-circuit.mjs";
import * as fixed from "./fixed/recall-circuit.mjs";

const work = mkdtempSync(join(tmpdir(), "bp-ab-"));
cpSync(process.argv[2], work, { recursive: true });
const gen = buildGraphGeneration(work, { outDir: ".agent", persist: true });
const db = openStoreReadOnly(join(work, ".agent", "graph", "graph.db"));
const gid = gen.manifest.generationId;

function walk(d,out=[]){for(const e of readdirSync(d)){if(e==="node_modules"||e===".git"||e===".agent")continue;
 const p=join(d,e),s=statSync(p); if(s.isDirectory())walk(p,out); else if(/\.(mjs|js|ts)$/.test(e))out.push(p);} return out;}
const cases=[];
for (const f of walk(work)){ const rel=relative(work,f).replace(/\\/g,"/"); const L=readFileSync(f,"utf8").split(/\r?\n/);
 for(let i=0;i<L.length;i++){const m=/^export (?:async )?function ([A-Za-z_][A-Za-z0-9_]*)/.exec(L[i]); if(!m)continue;
  const name=m[1]; const doc=[];
  for(let j=i-1;j>=0&&doc.length<6;j--){const t=L[j].trim();
   if(t.startsWith("//")){doc.unshift(t.replace(/^\/\/\s?/,""));continue;}
   if(t.startsWith("*")||t.startsWith("/*")||t.startsWith("*/")){doc.unshift(t.replace(/^[*/ ]+/,""));continue;} break;}
  const prose=doc.join(" ").trim(); if(prose.split(/\s+/).length<6)continue;
  const parts=new Set(name.replace(/([a-z0-9])([A-Z])/g,"$1 $2").toLowerCase().split(/[^a-z0-9]+/).filter(Boolean));
  const nl=prose.split(/\s+/).filter(w=>!parts.has(w.toLowerCase().replace(/[^a-z0-9]/g,""))).join(" ").slice(0,180);
  cases.push({name,path:rel,nl});}}

function run(mod, task, c, limit=10){
  const circuit = mod.executeRecallCircuit(db, task, { generationId: gid });
  const set = mod.recallCircuitToCandidateSet(circuit);
  const rows = set.candidates.slice(0,limit);
  for(let i=0;i<rows.length;i++) if(String(rows[i].sourceRef).startsWith(c.path+":")) return {rank:i+1,state:circuit.state,n:rows.length};
  return {rank:0,state:circuit.state,n:rows.length};
}
const R={base:{ex:0,nl:0,exMrr:0,cand:0},fixed:{ex:0,nl:0,exMrr:0,cand:0}};
for (const c of cases){
  for (const [k,mod] of [["base",base],["fixed",fixed]]){
    const a=run(mod,c.name,c); if(a.rank){R[k].ex++;R[k].exMrr+=1/a.rank;}
    const b=run(mod,c.nl,c);   if(b.rank)R[k].nl++;
    R[k].cand+=a.n;
  }
}
const n=cases.length;
console.log(`cases=${n} graphNodes=${gen.nodes.length}`);
for (const k of ["base","fixed"])
  console.log(`${k.padEnd(6)} exact-id top10=${(R[k].ex/n*100).toFixed(1)}% mrr=${(R[k].exMrr/n).toFixed(3)}  NL top10=${(R[k].nl/n*100).toFixed(1)}%  avgCandidates=${(R[k].cand/n).toFixed(1)}`);
