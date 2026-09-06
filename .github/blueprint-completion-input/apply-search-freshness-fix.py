from pathlib import Path
import subprocess

ROOT = Path.cwd()
SERVICE = ROOT / "blueprint/src/lib/application/service.mjs"
BM25 = ROOT / "blueprint/src/graph/bm25-code-index.mjs"
WORKFLOW = ROOT / ".github/workflows/blueprint-completion.yml"
SELF = ROOT / ".github/blueprint-completion-input/apply-search-freshness-fix.py"

# A symbol-like node can be source-addressed only through evidence. BM25 must
# retain that canonical path or a stale generation can re-surface the symbol
# after the exact lane correctly suppresses it.
bm25_text = BM25.read_text()
bm25_old = '      path: node.path ?? "",\n'
bm25_new = '      path: node.path ?? node.evidence?.[0]?.path ?? "",\n'
if bm25_text.count(bm25_old) != 1:
    raise SystemExit(f"expected one BM25 document path site, found {bm25_text.count(bm25_old)}")
BM25.write_text(bm25_text.replace(bm25_old, bm25_new, 1))

service_text = SERVICE.read_text()
service_old = '''          bm25Projection.value.search(query, { limit }).map((row) => ({ ...row.document.node, lexicalScore: row.score, lexicalExactName: row.exactName })),
'''
service_new = '''          bm25Projection.value.search(query, { limit }).map((row) => ({
            ...row.document.node,
            // Freshness suppression is path-addressed. BM25 owns a canonical
            // source path even when the projected public node omits one.
            path: row.document.path || row.document.node?.path || row.document.node?.evidence?.[0]?.path || null,
            lexicalScore: row.score,
            lexicalExactName: row.exactName,
          })),
'''
if service_text.count(service_old) != 1:
    raise SystemExit(f"expected one BM25 mapping site, found {service_text.count(service_old)}")
SERVICE.write_text(service_text.replace(service_old, service_new, 1))

subprocess.run([
    "node", "--test",
    "tests/query-runtime-cluster.test.mjs",
    "tests/application-service-queries.test.mjs",
    "tests/application-v2-projections.test.mjs",
    "tests/retrieval-projections.test.mjs",
], cwd=ROOT / "blueprint", check=True)

subprocess.run(["git", "config", "user.name", "Blueprint completion automation"], cwd=ROOT, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=ROOT, check=True)
subprocess.run(["git", "add", str(SERVICE.relative_to(ROOT)), str(BM25.relative_to(ROOT))], cwd=ROOT, check=True)
subprocess.run([
    "git", "commit",
    "-m", "fix(blueprint-search): preserve source identity through BM25 enrichment",
    "-m", "Derive every BM25 document's source path from the canonical node path or source evidence, then carry that path across result enrichment. This keeps stale-source suppression authoritative even for projection nodes whose compact public shape omits path metadata.",
], cwd=ROOT, check=True)

workflow = WORKFLOW.read_text()
workflow = workflow.replace("permissions:\n  contents: write\n", "permissions:\n  contents: read\n", 1)
step = '''      - name: Apply reviewed search freshness fix\n        run: python3 .github/blueprint-completion-input/apply-search-freshness-fix.py\n'''
if step not in workflow:
    raise SystemExit("completion workflow patch step missing")
WORKFLOW.write_text(workflow.replace(step, "", 1))
subprocess.run(["git", "rm", str(SELF.relative_to(ROOT))], cwd=ROOT, check=True)
subprocess.run(["git", "add", str(WORKFLOW.relative_to(ROOT))], cwd=ROOT, check=True)
subprocess.run(["git", "commit", "-m", "ci(blueprint): remove search freshness transport [skip ci]"], cwd=ROOT, check=True)
subprocess.run(["git", "push", "origin", "HEAD:blueprint-completion"], cwd=ROOT, check=True)
