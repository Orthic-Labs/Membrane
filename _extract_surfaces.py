#!/usr/bin/env python3
"""Scan 26 repos for code-only capability surface: MCP tools, CLI subcommands,
REST/gRPC routes, SDK exports. Emits file:symbol candidates grouped by repo+cat.
Read-only; does not modify repos.
"""
import os, re

REPOS = r"//192.168.1.7/d/claude/repos/membrane"
SKIP = {'.git','node_modules','dist','target','build','__pycache__','.venv','venv','engine',
        'out','.idea','.vscode','site-packages','.tox','eggs'}
EXTS = {'.py','.js','.ts','.tsx','.jsx','.go','.rs','.java','.scala','.kt','.rb','.mjs',
        '.cjs','.lua','.proto','.c','.h','.cpp','.hpp','.cs','.ex','.exs'}

MCP = re.compile(r'@\w*\.tool\s*\(|@mcp\.tool|fastmcp|\bmcp\.tool\(|@server\.tool|app\.tool\(|register.*tool', re.I)
CLI = re.compile(r'@click\.(command|group|option|argument)|add_parser\s*\(|add_subcommand\s*\(|add_command\s*\(|cobra\.Command|clap::|Subcommand|@cli\.command|@app\.command|defcli|typer', re.I)
ROUTE = re.compile(r'@app\.route|app\.(get|post|put|delete|patch)\s*\(|router\.(get|post|put|delete)\s*\(|RegisterHandler|HandleFunc|@(Get|Post|Put|Delete|Patch)Mapping|func\s*\([^)]*\)\s*ServeHTTP|@RequestMapping', re.I)
PROTO = re.compile(r'\brpc\s+\w+')
EXPORT_PY = re.compile(r'^(def|class|async def)\s+(\w+)')
EXPORT_TS = re.compile(r'export\s+(default\s+)?(function|class|const|interface|type)\s+(\w+)')
defn = re.compile(r'^\s*(async\s+)?def\s+(\w+)|^\s*(public|private|protected|internal)?\s*(static\s+)?(class|func|function|def|fn|defn|struct)\s+(\w+)')

cats = {'mcp':MCP,'cli':CLI,'route':ROUTE,'proto':PROTO}

def walk_files(root):
    for dp, dns, fns in os.walk(root):
        dns[:] = [d for d in dns if d not in SKIP]
        for fn in fns:
            if os.path.splitext(fn)[1].lower() in EXTS:
                yield os.path.join(dp, fn)

repo_order = sorted(d for d in os.listdir(REPOS) if os.path.isdir(os.path.join(REPOS,d)))
out = []
for repo in repo_order:
    rpath = os.path.join(REPOS, repo)
    hits = {'mcp':[], 'cli':[], 'route':[], 'proto':[], 'export':[]}
    counts = {'mcp':0,'cli':0,'route':0,'proto':0,'export':0}
    CAP = 90
    for f in walk_files(rpath):
        try:
            with open(f, 'r', encoding='utf-8', errors='ignore') as fh:
                lines = fh.readlines()
        except Exception:
            continue
        rel = os.path.relpath(f, rpath)
        for i, line in enumerate(lines):
            if len(line) > 600:
                continue
            for cat, pat in cats.items():
                if counts[cat] >= CAP:
                    continue
                if pat.search(line):
                    sym = ''
                    m = defn.search(line)
                    if m:
                        sym = m.group(m.lastindex) or ''
                    if not sym and i+1 < len(lines):
                        m2 = defn.search(lines[i+1])
                        if m2:
                            sym = m2.group(m2.lastindex) or ''
                    if not sym:
                        q = re.search(r'["\']([a-zA-Z0-9_\-]{2,40})["\']', line)
                        if q and cat in ('cli','proto','route'):
                            sym = q.group(1)
                    tag = f"{rel}:{sym}" if sym else rel
                    hits[cat].append(tag)
                    counts[cat]+=1
                    break
            if counts['export'] < CAP and (rel.endswith('__init__.py') or os.path.basename(rel) in ('index.ts','index.js')):
                if EXPORT_PY.search(line):
                    hits['export'].append(f"{rel}:{EXPORT_PY.search(line).group(2)}")
                    counts['export']+=1
                elif EXPORT_TS.search(line):
                    hits['export'].append(f"{rel}:{EXPORT_TS.search(line).group(3)}")
                    counts['export']+=1
    out.append(f"\n##### REPO {repo}")
    for cat in ('mcp','cli','route','proto','export'):
        if hits[cat]:
            out.append(f"  [{cat}] ({counts[cat]})")
            for h in hits[cat][:CAP]:
                out.append(f"    - {h}")

text = "\n".join(out)
with open(r'D:/Claude/membrane/_surfaces.txt','w',encoding='utf-8') as fh:
    fh.write(text)
print(f"Scanned {len(repo_order)} repos. Output lines: {len(out)}. Wrote _surfaces.txt")
