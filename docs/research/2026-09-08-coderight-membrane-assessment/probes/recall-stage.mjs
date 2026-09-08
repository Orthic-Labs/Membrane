// Stage attribution: where does exact-identifier recall actually fail?
import { readFileSync, readdirSync, statSync, mkdtempSync, cpSync } from "node:fs";
import { tmpdir } from "node:os"; import { join, relative } from "node:path";
import { buildGraphGeneration } from "../../../../blueprint/src/graph/static-provider.mjs";
import { openStoreReadOnly } from "../../../../blueprint/src/graph/store-sqlite.mjs";
import * as base from "../../../../blueprint/src/graph/recall-circuit.mjs";
import * as fixed from "./fixed/recall-circuit.mjs";
const work=mkdtempSync(join(tmpdir(),"bp-s-")); cpSync(process.argv[2],work,{recursive:true});
const gen=buildGraphGeneration(work,{outDir:".agent",persist:true});
const db=openStoreReadOnly(join(work,".agent","graph","graph.db")); const gid=gen.manifest.generationId;
function walk(d,o=[]){for(const e of readdirSync(d)){if(e==="node_modules"||e===".git"||e===".agent")continue;const p=join(d,e),s=statSync(p);
 if(s.isDirectory())walk(p,o); else if(/\.(mjs|js|ts)$/.test(e))o.push(p);}return o;}
const cases=[];
for(const f of walk(work)){const rel=relative(work,f).replace(/\\/g,"/");const L=readFileSync(f,"utf8").split(/\r?\n/);
 for(let i=0;i<L.length;i++){const m=/^export (?:async )?function ([A-Za-z_][A-Za-z0-9_]*)/.exec(L[i]);if(!m)continue;
  const doc=[];for(let j=i-1;j>=0&&doc.length<6;j--){const t=L[j].trim();
   if(t.startsWith("//")){doc.unshift(t);continue;} if(t.startsWith("*")||t.startsWith("/*")||t.startsWith("*/")){doc.unshift(t);continue;} break;}
  if(doc.join(" ").trim().split(/\s+/).length<6)continue; cases.push({name:m[1],path:rel});}}
for (const [label,mod] of [["base",base],["fixed",fixed]]) {
  let seedOk=0, candOk=0, seedOkCandMiss=0;
  for (const c of cases) {
    const circuit = mod.executeRecallCircuit(db, c.name, { generationId: gid });
    const inSeeds = circuit.seeds.some(s => s.id === `symbol:${c.path}::${c.name}`);
    const set = mod.recallCircuitToCandidateSet(circuit);
    const inCand = set.candidates.slice(0,10).some(x => String(x.sourceRef).startsWith(c.path+":"));
    if (inSeeds) seedOk++;
    if (inCand) candOk++;
    if (inSeeds && !inCand) seedOkCandMiss++;
  }
  const n=cases.length;
  console.log(`${label}: correct symbol resolved as SEED ${(seedOk/n*100).toFixed(1)}%  |  present in delivered candidates ${(candOk/n*100).toFixed(1)}%  |  found as seed but DROPPED from candidates ${(seedOkCandMiss/n*100).toFixed(1)}%`);
}
