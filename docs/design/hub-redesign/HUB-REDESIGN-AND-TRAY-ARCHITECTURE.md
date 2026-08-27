# Membrane Hub — redesign and tray architecture

**Status:** proposal · not landed
**Date:** 2026-08-27
**Scope:** `apps/membrane-hub` frontend + `src-tauri`, macOS and Windows
**Companion artifacts:** [`dashboard.html`](dashboard.html) (approved Overview) · [`hub-mockup.html`](hub-mockup.html) (popover / tray / first-run plates) · [`MEMBRANE-BRAND-IDENTITY.md`](MEMBRANE-BRAND-IDENTITY.md)

Nothing here is landed. Per Membrane's completion rule, a capability is landed only when the
production path executes it and frozen acceptance evidence shows it meets the baseline it
replaces. This document is the plan and the evidence that motivates it.

---

## 1. What is actually wrong today

Evidence gathered on this machine, 2026-08-27.

### 1.1 The Windows popover is positioned off-screen (root cause of "the tray doesn't work here")

`apps/membrane-hub/src-tauri/src/main.rs:406`

```rust
fn popover_origin(icon: tauri::Rect, window_width: u32, scale: f64) -> (i32, i32) {
    let position = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let x = (position.x + size.width / 2.0 - f64::from(window_width) / 2.0).round() as i32;
    let y = (position.y + size.height).round() as i32;   // <- always BELOW the icon
    (x, y)
}
```

`toggle_popover` (`main.rs:414`) then clamps x to both screen edges but clamps y on **one** side
only:

```rust
y = y.max(top);        // top edge guarded; bottom edge is not
```

The macOS menu bar is at the top of the screen, so "below the icon" is correct there. The Windows
notification area is at the **bottom** of the screen. On a 1440p display the tray rect sits at
roughly `y = 1400`, so the popover's top edge is placed at ~`1440`, and a 330 px window is drawn
entirely past the bottom of the display. Nothing appears, and because the window did technically
show and then lost focus, `WindowEvent::Focused(false)` hides it again.

The unit test at `main.rs:1273` asserts only the top-anchored case, which is why this survived:

```rust
assert_eq!(popover_origin(icon, 300, 1.0), (-38, 24));
```

Corroborating runtime evidence — `%APPDATA%\com.membrane.hub\hub.jsonl` records three
`popover / hidden` events with no matching visible interaction, consistent with show-then-hide.

**This is the defect the user reported, and Codex did not identify it.**

### 1.2 Windows startup registration is not implemented

`main.rs:621`

```rust
#[cfg(not(target_os = "macos"))]
fn set_platform_startup(_enabled: bool) -> Result<(), String> {
    Err("membrane_hub_startup_unsupported".into())
}
```

macOS writes a LaunchAgent plist (`main.rs:597`). Windows has no equivalent. `index.html` catches
the error and sets `startup.disabled = true`, which is why the dashboard checkbox is dead on this
platform.

### 1.3 First run shows nothing

`tauri.conf.json` declares both windows `"visible": false`, and `setup()` calls `w.hide()` and
`dashboard.hide()`. A first install therefore produces no visible surface at all. The app assumes
the tray icon is discoverable.

### 1.4 Windows 11 hides new notification-area icons by default

New tray icons are filed behind the overflow chevron until the user drags them out. The Hub ships
no first-run affordance explaining this, so even with §1.1 fixed the icon is invisible to a new
user. (Stated as known platform behaviour; the per-machine registry state was not inspected.)

### 1.5 The tray asset is a single Windows size

`main.rs:383` selects `-36.png` on macOS and `-32.png` everywhere else. Windows requests
`SM_CXSMICON` (16 px at 100 % DPI, 24 px at 150 %, 32 px at 200 %) and downscales whatever it is
given. A single 32 px asset is soft at 100 % and 150 %.

### 1.6 Qualification uninstalls the build it just proved

`scripts/qualification/install-release.ps1:1288` runs `uninstall.exe /S` and
`Assert-UninstallResidue` requires the install root to be gone. Qualification is a clean-room
proof by design; it is not a deployment, and it leaves the machine with no Hub. Treating a passing
qualification run as "dogfood installed" is the mistake.

### 1.7 Design defects in the current UI

Measured against the Legion designer slop catalog, the shipped `src/overview.css` contains:

| Defect | Where | Rule |
|---|---|---|
| `radial-gradient` on `body`, `linear-gradient` on `.brand>b`, `.metric`, `.panel` | overview.css | zero-gradient rule (user), `ai-color-palette` |
| Side-stripe border: `.metric:before` 2 px coloured left edge | overview.css | `border-accent-on-rounded` (absolute ban) |
| Glow shadow: `box-shadow: 0 0 12px rgba(84,214,154,.45)` on the state pill | overview.css | `dark-glow` |
| Tracked uppercase eyebrow on every panel (`MEMORIES`, `LEDGER`, `SOURCES`, `HEALTH`…) | overview.mjs | `repeated-section-kickers` |
| Hero-metric template: six-up metric cards with big numeral + small label | overview.mjs | hero-metric cliché |
| `transition:.16s` (unqualified shorthand) | overview.css | `transition: all` |
| State conveyed by dot colour alone | overview.mjs | colour-only state indicator |
| Generic token names `--surface`, `--surface2` | overview.css | token-naming default |
| No loading, empty, disabled or recovery states; one bare "Offline" card | overview.mjs | state completeness |

---

## 2. Codex report triage

| # | Codex claim | Verdict | Evidence |
|---|---|---|---|
| 1 | Qualification uninstalled Hub, then claimed completion | **Valid** | `install-release.ps1:1288-1292` uninstalls; `Assert-UninstallResidue` requires a clean root |
| 2 | First launch hides both dashboard and popover | **Valid** | `tauri.conf.json` `visible:false` ×2; `setup()` hides both |
| 3 | Windows may put the icon in overflow; no first-run explanation | **Valid as a gap**; platform default not machine-verified | no first-run code path exists |
| 4 | Windows `set_platform_startup` returns `membrane_hub_startup_unsupported` | **Valid** | `main.rs:621` |
| 5 | The dashboard checkbox therefore disables itself | **Valid** | `index.html` startup error handler |
| 6 | Tests prove the checkbox exists, not that startup works | **Valid** | `tests/*.mjs` cover presentation only |
| 7 | Qualification proved process behaviour, not a visible tray icon | **Valid** | no rendered or UI-automation tray evidence in the suite |
| 8 | Start Menu entry exists; uninstall registry entry not found | **Partly verified** | `Membrane Hub.lnk` present in Start Menu; registry read was not permitted in this session, so the second half is unconfirmed |
| — | "Opening the Start Menu shortcut while Hub runs must show the dashboard" | **Already implemented, not a repair** | `main.rs:694` — `tauri_plugin_single_instance::init(\|app, _, _\| { show_dashboard(app); })`. This is how the second launch opened the dashboard |
| — | Membrane runtime / Blueprint "processes" running | **Correct outcome, wrong mechanism** | `membrane-runtime` is a path dependency compiled into `membrane-hub.exe`; `tasklist` shows no `membrane.exe`/`cortex.exe`. `hub.jsonl` confirms `service_state=running`, `blueprint_installed=running` |

Codex's list is substantially accurate. It misses the actual positioning bug (§1.1), the DPI asset
gap (§1.5), and every design defect (§1.7), and it lists one already-implemented behaviour as a
repair item.

---

## 3. Target architecture

### 3.1 Port strategy: take HeardRight's shell, keep Membrane's producer

HeardRight (`D:\Claude\heardright\tauri-app-next`) already solves the frameless-shell problem on
both platforms and ships the suite's fonts. The port takes its **shell**, not its **app**.

| Layer | Source | Action |
|---|---|---|
| Frameless window + fused sidebar grid | `src/components/TitleBar.tsx`, `Sidebar.tsx`, `src/lib/ui.ts` | **Port the shell *contract*, not its literal geometry.** Keep the two-column × two-row grid, the per-OS chrome split, `WindowControls`, and `data-tauri-drag-region` placement. HeardRight currently uses a 208 px rail and a 40 px titlebar (`src/lib/ui.ts:12` — `[grid-template-columns:208px_1fr] [grid-template-rows:40px_1fr]`). Membrane uses its own shell tokens, currently **212 px / 46 px** in the approved mockup. Do not treat HeardRight's numbers as Membrane's. |
| Platform branching | `hooks/usePlatform.ts` (`@tauri-apps/plugin-os`) | **Port as-is.** `membrane-hub` already depends on `@tauri-apps/plugin-os` |
| Fonts | `public/fonts/Tanker-400.woff2`, `SplineSansMono-*.woff2` | **Copy into `apps/membrane-hub/assets/fonts/`**, add to `PRESENTATION_ASSETS` |
| Startup registration | `tauri-plugin-autostart` v2 (HeardRight `Cargo.toml:103`) | **Adopt**, replacing the hand-rolled `set_platform_startup` (§3.3) |
| Single instance | already present in `membrane-hub` | keep |
| Theme tokens, ember palette, HR copy, pill, engine, licensing | HeardRight | **Do not port.** Membrane's identity and producer are its own |
| Snapshot model, envelope parsing, lifecycle reason labels, `dashboardModel` | `src/overview.mjs`, `src/popover.mjs` | **Keep unchanged.** These encode frozen producer semantics and their tests must keep passing |

#### Toolchain: vanilla renderer, Vite + Tailwind v4 build infrastructure

Keep Membrane's renderer vanilla; do not port React or HeardRight application state. **Adopt Vite +
Tailwind CSS v4 as build-time frontend infrastructure** so the HeardRight shell can be ported
faithfully and maintained in the same utility vocabulary. Membrane retains its own HTML/ES-module
application architecture, CSP, producer model, design tokens and tests.

Two separable decisions, previously conflated:

1. **Do not import React to get the shell.** Port the *layout contract* — grid areas, per-OS chrome,
   drag regions, roving-tabindex sidebar — not the component framework.
2. **Do adopt Tailwind.** Tailwind has no dependency on React; it is a build-time CSS system.
   HeardRight runs both plugins side by side (`vite.config.ts:42` — `plugins: [react(), tailwindcss()]`,
   `@tailwindcss/vite` ^4.3.3). Taking one does not oblige us to take the other. Hand-authoring
   several hundred lines of CSS to re-express a shell that is already written in Tailwind utilities
   is the more expensive path, not the safer one.

Route: **Vite + `@tailwindcss/vite`, vanilla TS/JS, no React.** The Hub's existing ES modules move
through Vite; `scripts/build-frontend.mjs` keeps ownership of sidecar staging and release identity
and stops hand-copying vendored `@tauri-apps` modules. The alternative — keeping the current
copier and adding the Tailwind v4 CLI to emit one stylesheet — is a smaller migration and stays
available if the Vite move proves disruptive, but the shell is being substantially rewritten
anyway, so this is the moment when standardising costs least.

The security posture is unchanged. Tailwind emits static local CSS with no runtime; `style-src
'self'` remains valid, no CDN and no `unsafe-inline` are required, and `freezePrototype` is
unrelated to any of this.

Keep a small hand-written stylesheet for what utilities express badly: the filled / half / hollow /
dashed verdict glyphs, the SVG-mask brand mark, native-window edge cases, and a few platform
selectors. There is no virtue in forcing every rule into a utility class.

### 3.2 Window model

| Surface | Container | Chrome |
|---|---|---|
| Dashboard | Tauri window `dashboard` | Per-OS, and the two are **not** the same setting (see below) |
| Popover | window `hub`, `alwaysOnTop`, `skipTaskbar`, undecorated | Anchored to the tray rect, arrow flips with the anchor edge (§3.4) |
| First run | the `dashboard` window, opened once on a first-run marker (§3.5) | Same chrome |

**macOS.** Keep native decorations. `decorations(true)` + `TitleBarStyle::Overlay` + `hidden_title(true)` + full-size content view, so app chrome extends beneath the real titlebar and the native traffic lights stay native. Reserve a ~78 px gutter in the chrome row's left cell to clear them.

**Windows.** `decorations(false)`, with custom caption controls rendered in the webview at the right of the chrome row. Close hides to the notification area.

This is exactly what HeardRight does — `src-tauri/src/app_lib_sections/section05.rs:1188-1192`:

```rust
#[cfg(target_os = "macos")]
let builder = builder
    .decorations(true)
    .title_bar_style(tauri::TitleBarStyle::Overlay)
    .hidden_title(true);
#[cfg(not(target_os = "macos"))]
let builder = builder.decorations(false);
```

An earlier draft of this document said `decorations: false` on both platforms *and* `titleBarStyle: Overlay` on macOS. Those are mutually exclusive: Overlay is a decoration style, so it requires decorations to remain on.

Never fake macOS traffic lights on Windows.

### 3.3 Startup registration, both platforms

Replace both `#[cfg]` arms with one implementation. Two viable routes:

**Route A (preferred) — `tauri-plugin-autostart` v2.** Already proven in HeardRight on both
platforms. On Windows it writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; on macOS it
manages a LaunchAgent. Per-user, no elevation.

**Route B — hand-rolled Windows arm** mirroring the existing macOS arm, writing the same `Run`
value directly. Chosen only if the plugin's macOS plist shape conflicts with the current
`STARTUP_AGENT_LABEL` contract, which the migration must check before switching.

Either way:

- The registered command must carry a login marker argument, matching HeardRight's
  `--login-launch` convention (`login_launch.rs:21`), so the Hub can tell a login start from a
  human double-click and keep the first-run window suppressed at login.
- `startup_setting()` must **read back** the real registration rather than a cached preference, so
  a user who removes the entry through Task Manager sees the toggle follow.
- The typed error vocabulary stays: on genuine failure return a typed reason, never
  `membrane_hub_startup_unsupported` as a blanket platform answer.

### 3.4 Tray and popover, per platform

```
anchor_edge(icon_rect, monitor) -> Below | Above
    Below  when the icon sits in the TOP third of the work area   (macOS menu bar)
    Above  when the icon sits in the BOTTOM third of the work area (Windows taskbar)
```

```rust
fn popover_origin(icon: Rect, win: PhysicalSize, monitor: Monitor, scale: f64)
    -> (PhysicalPosition, Anchor)
```

Rules:

1. Compute the anchor from the icon rect against the monitor **work area**, not the full monitor
   rect, so a left/right/top Windows taskbar also resolves correctly.
2. Clamp on **all four** edges. The current code clamps three.
3. Return the anchor to the frontend (window event or query) so the CSS arrow flips:
   `.pop[data-anchor="below"]` draws the arrow on top, `[data-anchor="above"]` on the bottom. The
   mockup implements both.
4. Suppress the `Focused(false)` auto-hide for a short grace window after `show()`, or gate it on
   the popover having actually received focus. On Windows the shell can steal focus during the
   tray click and hide the window before the user sees it.
5. Ship the Windows tray icon at 16, 20, 24 and 32 px and select by the window's current scale
   factor, instead of always handing Windows the 32 px asset.

### 3.5 First run

```
on start:
  marker = app_data_dir()/first-run.json
  if !marker.exists() and !is_login_launch(argv):
      show dashboard on the first-run view
      write marker { shownAtUnixMs, version }
  else:
      stay tray-only
```

The first-run view is the dashboard window rendering one extra block: where the tray icon went,
how to pin it out of the Windows overflow, and the launch-at-login toggle. It is a real state of
the dashboard, not a separate wizard window. Later launches are tray-only; the Start Menu
shortcut already routes through `single_instance` to `show_dashboard`.

### 3.6 Design system

Full token table and rationale in `MEMBRANE-BRAND-IDENTITY.md`. The rules that bind the
implementation:

- **Zero gradients.** Flat fills only, everywhere, including the mark and the tray glyph.
- **Tanker** carries the wordmark, page titles, and empty-state headlines only. Chrome, list rows,
  tables, labels and menus use the system UI face (`native-app.md` §5). **Spline Sans Mono**
  carries evidence strings, identifiers, timestamps and counts.
- **The admission ledger is the workspace signature.** Every fact the Hub reports renders as
  `subject · verdict · evidence · observed`. The verdict is a shape plus a word (filled / half /
  hollow / dash), so it survives greyscale and colour-vision deficiency. The evidence column
  prints the typed reason verbatim (`graph_missing`, `not_instrumented`, `hub_inactive`) rather
  than a paraphrase — this is also what makes the UI honest about omissions, which is Membrane's
  own contract.
- Delete: the side-stripe metric cards, the per-panel eyebrows, the glow shadows, the six-up hero
  metric grid, the unqualified `transition` shorthand.
- Every failure names the failed subsystem and offers the action that clears it.

---

## 4. Acceptance

Nothing below may be claimed without frozen evidence.

### 4.1 Rust unit tests (`src-tauri`)

- `popover_origin` returns `Below` + a top-anchored origin for a top-of-work-area icon rect.
- `popover_origin` returns `Above` + an origin whose bottom edge clears the taskbar for a
  bottom-of-work-area icon rect. **This is the regression test for §1.1.**
- All four edges clamp: left, right, top, bottom, at scale 1.0 and 2.0.
- Left-, right- and top-docked Windows taskbar rects resolve to a fully on-screen origin.
- Windows startup: `set(true)` → `query() == true` → `set(false)` → `query() == false`, against a
  redirected registry root or a temp hive so the test does not touch the developer's own `Run` key.
- First-run marker: absent → dashboard requested and marker written; present → tray-only; login
  launch with marker absent → tray-only.

### 4.2 Frontend tests (`tests/*.mjs`)

- The existing snapshot/envelope/lifecycle tests keep passing unchanged.
- New: every verdict renders a shape class as well as a colour class.
- New: degraded, unavailable, not-configured, loading, empty and hub-inactive states each render a
  named subsystem and a recovery action.

### 4.3 Rendered evidence

Route to `/audit-visual`. Required captures: dashboard running / degraded / hub-inactive /
loading, popover anchored below and above, first-run view, and the Windows caption buttons — each
in dark and light, macOS and Windows.

### 4.4 Human-visible tray proof

The gap Codex named correctly. Required: UI automation or a rendered capture showing tray icon →
popover visible on screen → dashboard, on Windows. Process-liveness assertions do not substitute.

### 4.5 Dogfood installation

Qualification uninstalls by design (§1.6). A separate, explicit step must install the exact signed
build and **leave it installed**, enable launch-at-login, and re-verify Hub, tray, popover,
dashboard, runtime and Blueprint after a real logout/login. Do not fold this into
`install-release.ps1`; that script's clean-room contract is correct and should not be weakened.

---

## 5. Sequence

1. Land the popover anchor fix and its tests (§3.4 rules 1–2, §4.1). Smallest change, fixes the
   reported defect.
2. Land Windows startup registration and its tests (§3.3).
3. Land the first-run marker and view (§3.5).
4. Stand up Vite + Tailwind v4 (§3.1), then port the shell and apply the identity (§3.6), keeping
   the producer model and its tests untouched.
5. Tray asset sizes and the focus-race guard (§3.4 rules 4–5).
6. Rendered and automation evidence (§4.3, §4.4).
7. Sign, install, leave installed, verify across a restart (§4.5).

Steps 1–3 are behaviour and can ship before the visual work. Step 4 is the largest diff and
touches no producer semantics.
