from pathlib import Path
import subprocess

ROOT = Path.cwd()
SERVICE = ROOT / "blueprint/src/lib/application/service.mjs"
WORKFLOW = ROOT / ".github/workflows/blueprint-completion.yml"
SELF = ROOT / ".github/blueprint-completion-input/apply-search-freshness-fix.py"

old = '''          bm25Projection.value.search(query, { limit }).map((row) => ({ ...row.document.node, lexicalScore: row.score, lexicalExactName: row.exactName })),
'''
new = '''          bm25Projection.value.search(query, { limit }).map((row) => ({
            ...row.document.node,
            // Freshness suppression is path-addressed. Preserve the BM25 document's
            // canonical source path even for projection nodes whose public node shape
            // omitted it; enrichment must never bypass the stale-source boundary.
            path: row.document.node?.path || row.document.path || null,
            lexicalScore: row.score,
            lexicalExactName: row.exactName,
          })),
'''
text = SERVICE.read_text()
if text.count(old) != 1:
    raise SystemExit(f"expected one BM25 mapping site, found {text.count(old)}")
SERVICE.write_text(text.replace(old, new, 1))

subprocess.run([
    "node", "--test",
    "tests/query-runtime-cluster.test.mjs",
    "tests/application-service-queries.test.mjs",
    "tests/application-v2-projections.test.mjs",
    "tests/retrieval-projections.test.mjs",
], cwd=ROOT / "blueprint", check=True)

subprocess.run(["git", "config", "user.name", "Blueprint completion automation"], cwd=ROOT, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=ROOT, check=True)
subprocess.run(["git", "add", str(SERVICE.relative_to(ROOT))], cwd=ROOT, check=True)
subprocess.run([
    "git", "commit",
    "-m", "fix(blueprint-search): preserve source path through BM25 enrichment",
    "-m", "Keep BM25-enriched candidates addressable by the canonical stale-source boundary. A changed source must remain suppressible even when an enrichment projection omits path metadata from its public node shape.",
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
