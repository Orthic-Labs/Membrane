> Converted from PDF (pymupdf4llm); equations and figures are lossy. Cite the source.

###### `GLITCH CAT CLUB` 

# Applying Knowledge Graphs 

How to add a knowledge graph to an AI system, with the correct tooling. 

##### **This artefact will demonstrate:** 

1. A knowledge graph made the smallest model as accurate as the biggest: Haiku went from a wrong answer to the right one, with **zero tool calls** and **2 ms** retrieval. 

2. Faster and cheaper, measured: 20 seconds of visible searching became an instant answer; 660 to 1,180 tokens of document reads became ~400 tokens, **fixed at any corpus size** . 

3. Proven on a deliberate multi-hop trap case. 

4. The mechanism: deterministic prompt injection through a `UserPromptSubmit` hook, so retrieval bypasses the model entirely; the traversal is done before the model wakes. 

5. How to build your own: the ontology and logical modelling are covered below, and the repo teaches your AI the whole design system. 

```
ALL RUNS REAL · captured 16 Aug 2026
```

```
REPO · github.com/Glitch-Cat-Club/graph-memory-starterVIDEO · walkthrough coming
```

```
01 · THE TEST CASE
```

## **A trap, on purpose** 

**What it proves:** with a graph, even the smallest model answers a question that defeats keyword search. 

#### **The trap:** 

1. The rule lives in one file: refunds over £500 need the Ops Manager. 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 1 of 19 

2. The person lives in another: the Ops Manager is Sarah. 

3. The twist lives in a third: Sarah is away all March; Marcus covers her. 

4. Decoys planted around them, like every real folder has: a stale policy with different numbers, an expenses doc using the same £500. 

The three files share no words, so keyword search finds the wrong things first. That's the point. 

**How we prove it:** the same question, two folders (one raw, one with the graph), three model sizes, with and without. Every file ships in the <u>repo; run it all yourself.</u> 

**"A customer wants an £800 refund in March. Who signs it off?"** Correct answer: Marcus Webb. 

#### **The rest of this artefact:** 

The difference: pull vs push 

What Tier 1 is, and the tiers above it 

The results in detail 

How to build an ontology, loosely 

The store: as many tables as your facts have shapes 

Traversal: the two jobs and the code 

The injection: how it works 

```
02 · THE DIFFERENCE
```

## **Pull vs push** 

**The pretend knowledge graph** (most "graph" demos): the model still does the searching and reading itself; the graph is decoration. **Done properly:** code finds the facts before the model is involved and hands them over; the model never takes part in the finding. 

PULL · search during the turn 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 2 of 19 



<!-- Start of picture text -->
prompt model grep read grep model answer<br>seconds · cost grows with corpus size · accuracy depends on the model<br>PUSH · retrieve before the turn<br>prompt hook queries the graph inject model answer<br>2 ms · no LLM<br>one model call · fixed cost · accuracy from structure<br><!-- End of picture text -->

Pull: the model hunts for facts during the conversation. Push: code finds the facts first and hands them over. That swap is the whole difference. 

```
03 · GETTING STARTED
```

## **There are tiers to this. Today: Tier 1.** 

Levels exist so you don't have to go deep to get value. Tier 1 is enough to start properly. 

|**TIER**|**TOOLING**|**WHEN**|
|---|---|---|
|**Tier**|SQLite or Postgres. A real graph in three|Up to a few hundred thousand|
|**1** ←|tables, traversed by a recursive query.|facts. Most personal and small-|
|this||business systems live here.|
|demo|||
|**Tier**|An embedded graph engine: Cypher and|Deep, variable-length queries.|
|**2**|native traversal, one file, no server. Kuzu<br>lineage (archived 2025; forks continue).||
|**Tier**|Neo4j. The enterprise standard; traversal runs|Team and server.|
|**3**|inside the engine.||



One rule at every tier: **the model never does the traversal.** Code queries the graph. The model reads the result. 

What Tier 1 is, in full: 

- 3 SQLite tables: entities, relations, aliases 

- 1 recursive query for traversal 

- 1 prompt-submit hook for injection 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 3 of 19 

~250 lines. No server, no API key, no framework. 

###### `04 · RESULTS` 

## **The graph made the smallest model as accurate as the biggest** 

**The headline: without the graph, Haiku named the wrong approver. With it: correct, zero tool calls, 2 ms retrieval.** The model stopped mattering for correctness, because the graph walks the facts and the model only reads them. The reasoning moved upstream: a strong model builds the graph once; a cheap model answers everything after. 

Detail, if you want it: 

|**MODEL**|**SEARCH (NO GRAPH)**|**GRAPH (INJECTED)**|
|---|---|---|
|Fable 5|correct · 2 searches, 4 files read, 20 s|correct · 0 tool calls|
|Sonnet|correct · 5 searches, 8 files read, 13 calls|correct · 0 tool calls|
|Haiku|**wrong**· stopped at the first fact|**correct**· 0 tool calls|



- `❯ A customer wants an £800 refund in March. Who signs it off?` 

- ⎿ `UserPromptSubmit says: memory: 8 facts recalled in 2 ms` 

- `Marcus Webb. Over the £500 threshold, so Ops Manager sign-off; that role is Sarah Chen's; she is on leave all March and the delegation memo gives Marcus her approvals for that window.` 

- Injected context: ~400 tokens, fixed at any corpus size. Search reads: 660 to 1,180 tokens, growing with scale. 

- Unscripted questions on Haiku + graph: all correct. Outside the vocabulary: "no memory matches", no guessing. 

- One question, one corpus, six runs. The mechanism is what generalises; the repo re-runs everything 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 4 of 19 

re-runs everything. 

#### **What happens as chains get longer** 



<!-- Start of picture text -->
COST PER QUESTION ● graph (injected) ╌  search (model-driven)<br>search: every hop adds<br>reads and calls<br>measured: 660–1,180 tokens, 5–13 calls<br>graph: ~400 tokens, any depth<br>measured: ~400 tokens, 0 calls<br>past top-k depth: ranking needs tuning<br>CHANCE THE CHAIN COMPLETES<br>measured: 3 of 3, every model tier<br>measured: strong models, 3 of 3<br>search · strong model<br>measured: Haiku, 1 of 3<br>search · small model<br>1 2 3 4 5 6<br>chain depth (hops)<br><!-- End of picture text -->

Dots are measured (the 3-hop chain). Curves are the mechanism's shape: each search hop adds calls, reads and a vocabulary guess; the walk is one query at any depth. The dotted dip is the one honest graph caveat: past top-k depth, ranking needs tuning. 

#### **Limits, and what fixes each** 

|**LIMIT**|**WHEN YOU FEEL IT**|**FIX**|**NATURE**|
|---|---|---|---|
|Lexical|A question that names no|Add embedding seeding|Extension,|
|seeding|known entity or alias finds<br>nothing|beside the alias table|same<br>design|
|Top-k|Dense hub entities push chain|Rank by path membership|Tuning,|
|crowding|links out of a fixed-size injection|between seeds; prune; raise|same|
|||k|design|
|Store|Past a few hundred thousand|Move up a tier. Same|Tier move|
|size|edges, or heavy concurrent|schema shape||



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 5 of 19 

|size|edges, or heavy concurrent<br>queries|schema shape||
|---|---|---|---|
|Meaning|Name collisions merge; stale|Disambiguation at|Modelling|
|at scale|docs pollute|extraction; supersession at<br>ingest|discipline|



The limit is never hops. Six hops is six edges. Density, size, vagueness and ambiguity are the real axes, and nothing dead-ends. 

### **That is the proof.** 

The build is below. Feed the repo to your AI assistant and it learns the design system; apply it to your own docs. Video walkthrough coming, and more on taking this principle further. 

```
05 · BUILD: THE ONTOLOGY AND THE LOGICAL MODEL
```

## **How to build an ontology** 

Loosely, the journey: start from the questions you want answered. List the kinds of things those questions mention; that list is your entity types. List the ways those things connect; that list is your relationship types. Keep both lists small and closed, and put conditions (amounts, dates, windows) in the entity descriptions. That's the ontology. The logical model is then the shape a fact takes: a typed entity, a typed relation between two entities, an alias for a name variant. 

Our use case was company ops docs, and our questions were about approvals, roles and cover. Five entity types and five relationships fell straight out of them. Then we had the AI extract every doc against that vocabulary. Same journey for any domain: questions, then vocabulary, then extraction. Your AI assistant does the heavy lifting; the vocabulary decisions are yours. 

**ENTITY TYPES RELATIONSHIP TYPES** `PERSON` · `ROLE` · `POLICY` · `approved_by` · `held_by` · `delegates_to` · 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 6 of 19 

`part_of` · `references` 

`PROCESS` · `DOCUMENT` 

```
corpus/refund-policy.md · each doc carries the vocabulary in front matter
```

```
---
type: policy
title: Refund approvals
entities: [Refund approvals, Ops Manager]
links: [org-chart, customer-support-sop]
---
Refunds over £500 require approval by the **Ops Manager**
before payment is released.
```

Extraction works on unstructured prose too. The vocabulary is the prerequisite; front matter is maintenance discipline. 

```
06 · BUILD: THE STORE
```

## **As many tables as your facts have shapes** 

This is logical modelling: list the shapes your facts take, and each shape becomes a table. Our model needs three: a thing exists (entities), two things connect (relations), a thing has another name (aliases). Three is not a magic number; it is what this model needs. Facts in more shapes, like time-bounded facts or measurements, mean more tables, one per shape. 

```
src/schema.sql
```

```
CREATE TABLE entities  (id TEXT PRIMARY KEY,   -- uuid5(type + normalised
                        name TEXT, type TEXT,
                        description TEXT, source_doc TEXT);
CREATE TABLE relations (source_id TEXT, target_id TEXT,
                        predicate TEXT, source_doc TEXT);
CREATE TABLE aliases   (entity_id TEXT, alias TEXT);
```

**The headline: identity is computed, never looked up.** An entity's id is a hash of its type and name, so "Ops Manager" in two different docs becomes one node automatically No matching service no ML: 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 7 of 19 

automatically. No matching service, no ML: 

```
src/build_graph.py
```

```
def entity_id(type_: str, name: str) -> str:
    key = f"{type_}:{normalise(name)}"
    return str(uuid.uuid5(uuid.NAMESPACE_OID, key))
```

Run `python src/build_graph.py` : 13 entities, 13 relations, 10 aliases from 8 docs. Extraction varies run to run; the hash is why re-runs merge instead of duplicating. 

```
07 · BUILD: TRAVERSAL
```

## **Two jobs: find the starting points, walk the connections** 

Every implementation needs these two jobs, whatever the tier. Seeding: match the question's words against entity names and aliases. Walking: collect everything connected to the seeds, hop by hop. In Tier 1 both jobs live in one small file ( `recall.py` ), and the walk is the query below. At Tier 2 and 3 the same walk becomes one line of Cypher; the jobs never change. 



<!-- Start of picture text -->
"£800 refund in March,<br>who signs it off?"<br>matches "refund"<br>delegates_to<br>Refund policy approved_by Ops Manager held_by Sarah Chen (March) Marcus<br>POLICY ROLE PERSON PERSON<br>a search for "refund" stops here<br><!-- End of picture text -->

The question seeds at "refund". The walk does the rest. 

```
src/recall.py · the walk (the repo adds a nearness ranking on top)
```

```
WITH RECURSIVE walk(entity_id, depth) AS (
  SELECT id, 0 FROM entities WHERE id IN ({seeds})
  UNION
  SELECT CASE WHEN r.source_id = w.entity_id
              THEN r.target_id ELSE r.source_id END,
```

```
wdepth+1
```

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 8 of 19 

```
         w.depth + 1
  FROM relations r JOIN walk w
    ON w.entity_id IN (r.source_id, r.target_id)
  WHERE w.depth < ?
)
SELECT e1.name, r.predicate, e2.name, r.source_doc
FROM relations r
JOIN entities e1 ON e1.id = r.source_id
JOIN entities e2 ON e2.id = r.target_id
WHERE r.source_id IN (SELECT entity_id FROM walk)
  AND r.target_id IN (SELECT entity_id FROM walk)
```

```
output · 2 ms (abridged, elision marked)
```

```
memory: 8 facts recalled in 2 ms
```

```
Refund approvals --[approved_by]--> Ops Manager     (refund-policy.md)
Ops Manager --[held_by]--> Sarah Chen               (org-chart.md)
Sarah Chen --[delegates_to]--> Marcus Webb          (delegation-memo.md)
… 5 more facts
```

```
where:
```

```
  Refund approvals: Refunds over £500 need Ops Manager sign-off; ...
  Sarah Chen: On leave 1-31 March
  Marcus Webb: Covers Sarah Chen's sign-offs during her March leave; ...
  … 6 more entity notes
```

The output is just text, shaped however you want; this shape carries the facts and their conditions. Whatever your stack, you always need the two jobs and a text output to hand the model. 

```
08 · BUILD: INJECTION
```

## **The whole integration is one JSON field** 

```
hooks.json → .claude/settings.json
```

```
{
```

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 9 of 19 

```
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python src/recall_hook.py",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
src/recall_hook.py
import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from recall import recall
prompt = json.load(sys.stdin).get("prompt", "")
facts = recall(prompt, hops=3, top_k=8)
text = facts.as_text()
print(json.dumps({
    "systemMessage": text.split("\n")[0],
    "hookSpecificOutput": {
        "hookEventName": "UserPromptSubmit",
        "additionalContext": text,
    }
}))
```

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 10 of 19 

`additionalContext` puts the facts in front of the model before it thinks. 

`systemMessage` prints the counts line at submit, before the model speaks. That's the entire mechanism. 

It fires on every prompt, in this project only. A prompt that matches nothing costs a millisecond and one "no memory matches" line; a match costs at most top-k facts (~400 tokens), and the model discards what it doesn't need. A relevance gate is the at-scale refinement. 

```
09 · MAKE IT YOURS
```

## **Clone, swap, rebuild** 

```
graph-memory-starter/
├── corpus/            8 modelled docs
├── corpus-before/     12 unstructured docs (the A/B control)
├── extraction/        LLM output per doc: nodes, edges, aliases
├── src/               schema.sql, build_graph.py, recall.py, recall_hook
├── hooks.json
└── extract-prompt.md  the extraction prompt, for your own docs
```

- Swap `corpus/` for your docs. Define your vocabulary from your questions. Extract with `extract-prompt.md` . Rebuild. 

- Needs: a coding assistant with prompt hooks, Python, SQLite. Nothing to install, no API key. 

- To reproduce the A/B exactly: run the search condition in `corpus-before/` before wiring the hook (or give that folder its own git root so the hook never reaches it), and pin the same model both sides with thinking off ( `.claude/settings.json` : `"model"` + `MAX_THINKING_TOKENS=0` ). 

```
APPENDIX
```

**The full loop** 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 11 of 19 

## **The full loop** 



<!-- Start of picture text -->
prompt turns + tool calls<br>user hits enter captured async, never blocks<br>query · ms<br>the store writes<br>hook extract<br>SQLite · entities / relations / aliases<br>recall_hook.py LLM · deferred: idle / threshold<br>(same shape at scale: Neo4j)<br>triples<br>model call<br>context pre-loaded<br>READ: deterministic, milliseconds WRITE: expensive, when nobody waits<br><!-- End of picture text -->

Capture is cheap and constant. Comprehension is expensive and deferred. 

#### **Run record** 

- Build: 13 entities, 13 relations, 10 aliases from 8 docs. Recall: 2 ms, full chain. Hook driven over stdin/stdout exactly as the harness drives it. Valid JSON out. 

- Nine fresh-agent probes and two live on-screen runs, 16 Aug 2026. The Haiku search failure and the honest empty-recall refusal included. 

https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 12 of 19 



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 8/17/26, 3:38 PM Page 13 of 19 



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 8/17/26, 3:38 PM Page 14 of 19 



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 8/17/26, 3:38 PM Page 15 of 19 



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 8/17/26, 3:38 PM Page 16 of 19 



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 8/17/26, 3:38 PM Page 17 of 19 



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 8/17/26, 3:38 PM Page 18 of 19 



https://claude.ai/code/artifact/5f055546-f494-4dc4-a677-677e9dfcfff4#no_universal_links 

8/17/26, 3:38 PM Page 19 of 19 

