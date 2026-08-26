# membrane-transcript

Native Rust transcript owner: deterministic `TranscriptEventV1` normalization
plus native discovery for Claude Code, Codex, CommandCode, Cline, OpenCode,
Qwen, Pi, Gemini, Grok Build, Roo-Cline, & Cursor raw stores. Frozen
`adapt_event_v1` snapshots remain accepted migration inputs.

Guarantees:

- deterministic stable event ids and digests (sha256, Python-compatible
  canonical JSON seeds so ids match the retired Python normalizer);
- byte-precise JSONL spans plus exact whole-document/database source binding;
- tool linkage (`call_id`, per-`call_id` occurrence, `toolCallEventId`
  back-reference from each `tool_result` to its `tool_call`);
- provenance on every event (host, sessionId, transcriptId, parserDigest);
- evidence flags (`synthetic`, `meta`, `privateReasoningOmitted`, `redacted`,
  `isError`, `isSidechain`);
- typed fail-closed errors, including readable sources that emit no events;
- `discover_open(home)` for native host stores; database discovery filters to
  open root OpenCode/Cursor sessions;
- no Python or Node runtime.

`discover_open(home)` supports parser conformance & evaluation only. Installed
`membrane adapt mine` requires caller-selected transcript paths.

Test only with RightKit:

```sh
rightkit cargo --manifest-path engine/crates/membrane-transcript/Cargo.toml test
```
