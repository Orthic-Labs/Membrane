# Troubleshooting

## Doctor states

`cortex doctor --full --json` returns a typed state:

| State | Meaning | First action |
|---|---|---|
| `ready` | fresh, complete, no blockers | none |
| `degraded` | fresh but partial coverage | review `reasons` |
| `stale` | graph older than source | `cortex build` |
| `broken` | artifacts present but inconsistent | `cortex doctor --repair-plan` |
| `corrupt` | artifacts unparseable | `cortex doctor --repair-plan` |
| `missing` | never built | `cortex build` |

## Repair plans

`cortex doctor --repair-plan --json` returns an ordered, non-destructive list
of repair actions. Preview before applying:

```sh
cortex doctor --repair-plan --json
cortex doctor --repair-plan --apply-repair --yes   # applies; requires --yes
```

Every action is `reversible: true` unless it is inherently non-reversible
(a rebuild regenerates the graph; it never destroys source).

## Support bundles

`cortex support-bundle <path>` writes a redacted diagnostic bundle:

- versions, installation, service/repository status
- doctor output and repair plan
- last bounded watcher/service log tails
- checksums for every record

Source content, absolute home paths, secrets, tokens, and raw environment
values are excluded. Path values are rewritten as `$HOME`, `$REPO`, or repo
IDs. Share the bundle with maintainers to diagnose install/watch/store
failures without leaking repository content.
