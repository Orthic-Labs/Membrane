# Membrane — brand identity

**Status:** proposed lock · awaiting Adrian's approval
**Date:** 2026-08-27
**Source:** created via `/brand-identity`. The private brand corpus
(`~/.claude/skills/brand/references/right-suite.md`) has **no Membrane row**; Membrane is an
unlocked identity, so this document creates one rather than loading one.
**Constraints given:** Tanker as the primary/display face (suite-wide lock, 2026-07-20), flat
black and purple, **no gradients under any circumstance**.

---

## 1. Brand truth

> Membrane helps people who build with AI agents get **only the evidence that actually earns a
> place in the model's attention**, by deciding admission and recording every omission, and must
> be recognised as **the boundary that keeps its receipts**.

Membrane is not a memory product and not a search product. It is the layer that says yes or no,
and can prove what it said no to.

## 2. Signature identity mechanism

> The identity is built around a **live admission boundary**: every surface states a subject, the
> verdict the boundary reached, and the literal evidence for that verdict — including what was
> held back.

How it behaves across channels:

- **Product UI.** One row grammar everywhere: `subject · verdict · evidence · observed`. The tray
  popover, the dashboard table, and the subsystem rail are the same row at three densities.
- **Marks and layout.** A single hairline is the boundary. Content sits on one side of it; the
  verdict sits on the line. No boxes-inside-boxes, because a card grid has no near side and far
  side.
- **Voice.** Membrane never paraphrases a machine reason. It prints `graph_missing`, then explains
  it in one sentence, then offers the action. The typed reason is the proof; the sentence is the
  courtesy.

Seven tests: brand-true (admission *is* the product), non-transplant (only a system that records
omissions needs an evidence column — paste it onto a CRM and it is false), nameable (the boundary
line and the verdict shape), systemizable (UI, docs, tray, CLI output), memorable (the four verdict
shapes), usable at 16 px monochrome (shapes, not hues), not the generic kit.

## 3. Colour

> The colour system makes **admission** legible by using a **near-black ground** so the verdict is
> the only thing that glows, an **accent reserved for command, selection and focus**, and **four
> verdict semantics that are shapes first and colours second**, differing from its siblings on
> hue family, chroma and role behaviour.

Every ratio below was measured with `color-check`, not estimated.

### Dark (default)

| Token | Hex | OKLCH (L/C/H) | Role | Measured |
|---|---|---|---|---|
| `--ink` | `#0A090C` | .143 / .007 / 301 | window ground | — |
| `--panel` | `#131117` | — | work surfaces | — |
| `--rail` | `#1A1721` | — | chrome, sidebar, rows | — |
| `--raised` | `#231F2C` | — | hover, selected | — |
| `--edge` | `#2C2836` | — | hairlines | 1.38:1 on ink |
| `--edge-strong` | `#3B3548` | — | active hairlines | — |
| `--text` | `#F2EFF6` | — | body | **17.45:1** on ink |
| `--dim` | `#9C96A8` | — | secondary | **6.94:1** on ink · 6.18:1 on rail |
| `--accent` | `#C56BFF` | .692 / .219 / 310 | command, selection, focus | **6.55:1** on ink · 5.82:1 on rail |
| `--accent-ink` | `#12071B` | — | text on accent fill | **6.46:1** |
| `--admit` | `#3FD98B` | .787 / .170 / 156 | verdict: admitted (**filled square**) | **10.89:1** |
| `--partial` | `#F0B23C` | .802 / .147 / 79 | verdict: degraded (**half square**) | **10.53:1** |
| `--refuse` | `#FF6B73` | .713 / .181 / 20 | verdict: refused (**hollow square**) | **7.19:1** |
| `--silent` | `#8A8598` | — | verdict: never configured (**dash**) | **5.57:1** |

### Light

| Token | Hex | Role | Measured on `--ink` |
|---|---|---|---|
| `--ink` | `#F6F4F8` | ground | — |
| `--panel` | `#FFFFFF` | surfaces | — |
| `--text` | `#1A1620` | body | **16.29:1** |
| `--dim` | `#655F72` | secondary | **5.60:1** |
| `--accent` | `#8A2BD6` | command | **5.66:1** · white on fill **6.18:1** |
| `--admit` / `--partial` / `--refuse` / `--silent` | `#0F7A47` / `#8A5A00` / `#C02734` / `#6A6478` | verdicts | 4.93 / 5.42 / 5.37 / 5.18 |

### Rules

- **No gradients.** Not in the UI, not in the mark, not in the tray glyph, not in marketing. This
  is an invariant, not a preference. It also removes the single most common AI-generated-UI tell.
- The accent appears on command, selection and focus. It is never a decorative wash and never
  encodes state — the verdict colours do that.
- **Colour never carries state alone.** Each verdict is a shape (filled / half / hollow / dash)
  plus a word. Greyscale, high-contrast, and red-green colour-vision deficiency all survive.
- Neutrals are violet-tinted (hue ~301 at C .007), not flat grey.

### Sibling differentiation

VoiceRight owns violet `#A78BFA` (OKLCH .709 / .159 / 294). Membrane's `#C56BFF` sits 16° further
toward magenta with 38 % more chroma: a saturated amethyst against VoiceRight's soft lavender. The
existing in-product purples `#9D69F5` (H 298) and `#A86BFF` (H 300) were *closer* to VoiceRight
than this is; moving to `#C56BFF` resolves a collision rather than creating one.

Against the rest of the suite: ScrapeRight and MailRight are the neutral-dark pair told apart by
gold vs red; CodeRight is black/grey with red; ViewRight is slate with blue; HeardRight and
VoiceRight share the coffee base. Membrane is the only violet-tinted near-black, the only
purple accent, and the only member whose accent is withheld from state entirely.

## 4. Typography

| Role | Face | Where |
|---|---|---|
| Display + wordmark | **Tanker** | wordmark, page titles, state headlines, empty-state headings |
| UI / body | **system face** — SF Pro on macOS, Segoe UI Variable on Windows | chrome, sidebar, list rows, tables, form labels, menus, buttons |
| Evidence | **Spline Sans Mono** | typed reasons, identifiers, generations, timestamps, counts |

Tanker is the primary brand face, as instructed and as the suite lock requires. It is confined to
display roles because `designer/references/native-app.md` §5 bans brand display faces in native app
chrome: a display face in list rows and menus loses optical sizing, the full weight range, and
correct localisation, and reads as a non-native app. Both constraints hold at once — Tanker is the
face people see and remember, the system face is the one they read at 13 px all day.

Spline Sans Mono is deliberate rather than a "tech flavour": Membrane's evidence strings are
literal machine tokens and must be visually distinct from prose that describes them.

## 5. Mark

The existing hex-brain glyph (`assets/tray/membrane-source@2x.png`) is retained as the source of
shape. It is recoloured to `--accent`, and its status variants are recoloured to the verdict
tokens. No gradient, no glow, no container tile. Construction rules stay in
`scripts/tray-icons.mjs`, which already crops to the alpha bounding box; the tray asset set extends
to 16 / 20 / 24 / 32 px for Windows DPI.

**Wordmark:** `Membrane`, one word, Tanker. Per the suite's "Right"-half convention the second half
takes the accent: `Mem` in `--text`, `brane` in `--accent`. Membrane is not a `*Right` product, so
this is an inherited rhythm, not an inherited name.

## 6. Voice

Precise, technical, evidence-first. Membrane states what is true, names the subsystem, prints the
reason, and offers the action.

**Owned:** admit, hold back, omission, receipt, evidence, generation, root, adapter, packet,
resident, verdict.
**Banned:** seamless, powerful, intelligent, effortless, supercharge, unlock, next-gen, "just
works", any sentence that describes Membrane's benefit without naming the mechanism.

Do:

> Blueprint has no graph for this root.
> Pull can still answer from Cortex, but nothing it returns is grounded in repository truth until
> Blueprint finishes a first scan.
> `repositories.state = unavailable · reason = graph_missing`

Don't:

> Something went wrong. Try again later.

Empty states name the condition and the first action that changes it. "No items found" is a
failure, not a state.

## 7. Restrictions

1. No gradients, anywhere, ever.
2. No glassmorphism, no glow shadows, no side-stripe borders, no nested cards.
3. No tracked uppercase eyebrow above sections. One named kicker as a system is voice; an eyebrow
   on every panel is AI grammar.
4. Tanker never appears in list rows, table cells, form labels, menus, or buttons.
5. The accent never encodes state; the verdict tokens never encode brand.
6. A verdict without its shape is not a verdict.
7. Machine reasons are printed verbatim and never paraphrased away.

## 8. Registry row

Append to the project registry when approved:

```markdown
| Membrane | Technical | Near-black (violet-tinted) | Display + system-native | Amethyst #C56BFF, withheld from state | Dense utility, hairline-divided | System mark (hex-brain) | UI-native | Precise, evidence-first | Live admission boundary: subject · verdict · evidence · observed, verdicts as shapes |
```

## 9. Open decisions

- Tagline: none. Do not invent one.
- Light theme is specified and measured but the app currently ships dark only; wiring the toggle is
  a separate change.
- Whether Membrane inherits the Right Suite lockup board treatment for marketing surfaces is
  unresolved and out of scope here.
