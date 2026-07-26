# Membrane

> **TL;DR:** Membrane gives AI coding agents useful context at the right moment—current code, project rules, past decisions & relevant memory—without stuffing everything into every prompt.

AI agents work better when they know what matters, but feeding them an entire workspace is slow,
expensive & noisy. Membrane is a local context layer that decides what an agent needs for each task,
where that information should come from & how much of it fits.

It combines three jobs:

- **Push:** compress active work, instructions & session state before context gets too large.
- **Pull:** retrieve relevant code, repository knowledge, rules & memories when a task needs them.
- **Persist:** keep durable decisions, preferences & lessons so future sessions do not start from zero.

Under the hood, Membrane uses typed context contracts, freshness checks, privacy gates, token
budgets, a local SQLite-backed memory engine called MemRight & federated providers such as
Blueprint. It gives each client a small, evidence-backed context packet instead of an uncontrolled
dump.

Membrane is not a chatbot or a generic vector database. It is infrastructure between an agent &
all sources that help that agent understand current work.

## Read next

- [`docs/UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md`](docs/UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md) — system boundary & design
- [`docs/MEMBRANE-STATE.md`](docs/MEMBRANE-STATE.md) — current implementation state
- [`engine/README.md`](engine/README.md) — MemRight engine
