# CodexBar — port assessment

**Status:** research finding · no code written · nothing landed
**Date:** 2026-08-27
**Question asked:** can CodexBar's tray/popover be ported and used for both OSes, or is it Swift — in which case, do we maintain Swift for macOS plus something else for Windows?

---

## Answer

**No Swift required. Do not maintain two stacks.**

There are two separate CodexBar codebases, and they share no UI code:

| | Repo | Language / framework |
|---|---|---|
| macOS | `github.com/steipete/CodexBar` | Swift 6.2 / SwiftPM, AppKit shell + SwiftUI content |
| Windows | `github.com/Finesssee/Win-CodexBar` | **Tauri 2 + Rust + React/TS** |

The Windows one is a third-party port, not steipete's. It shares zero UI code with the macOS app — it re-approximates the look in CSS inside a Tauri webview.

**Verdict: reproduce the design in Membrane's existing Tauri popover. Port no CodexBar code.**

Reasoning: the macOS app's entire look comes from being an `NSMenu` whose rows are `NSHostingView` SwiftUI views. Anchoring, dismissal, keyboard traversal, highlight and the glass material are all free from AppKit. None of that can leave AppKit, and none of it has a Windows analogue. The Windows port already faced this exact question and answered it by rebuilding in a webview — which is the stack Membrane Hub already runs.

Membrane Hub is at architectural parity with the Windows port today: same `TrayIconBuilder`, same undecorated always-on-top webview panel, same focus-loss hide. Reproducing the look is frontend work in the vanilla-TS/Tailwind rebuild — segmented bars, mini bar chart, key/value rows, chevron + shortcut action rows.

---

## How each one works

### macOS (`Sources/CodexBar/StatusItemController*.swift`)

- `NSStatusBar` status items, vended lazily per provider.
- The popover is **not** an `NSPopover` and not a custom window — it is a real **`NSMenu`** assigned to `statusItem.menu`, with `NSMenuItem`s carrying `NSHostingView` SwiftUI content (`MenuHostingView<Content>`, `MenuRowContentHostingView`).
- Progress bars, bar chart, KV rows and chevron/shortcut rows are SwiftUI views embedded as menu-item views.
- Anchoring and dismissal are free: AppKit positions the menu under the status item and handles outside-click/Esc, keyboard traversal and highlight.
- The glass is the **system menu material**, not hand-rolled. `NSVisualEffectView` appears only in preferences and for hosting-view backing.
- Size: status-item + menu-presentation layer ≈ 20k lines of Swift; irreducible tray plumbing ≈ 5–8k.

### Windows (`apps/desktop-tauri/src-tauri/src/`)

- `TrayIconBuilder::with_id("codexbar-main")` + `TrayIconEvent::Click` with the icon `rect` — the same API Membrane already uses.
- Rect + click position resolved against the monitor list into an anchor, then `window_positioner.rs` (573 lines, heavily unit-tested) computes the panel origin: centres on the icon, opens **above** for a bottom taskbar / below for a top taskbar, bottom-aligns for left/right taskbars, clamps to work area, DPI-aware.
- Panel is an undecorated always-on-top webview window. Dismissal is `Focused(false)` blur with a **500 ms post-tray-click grace window** plus a gesture guard so resize/drag doesn't self-dismiss.
- Borderless done the hard way: a DWM/Win32 subclass (`shell/dwm.rs`, 286 lines) intercepting `WM_NCCALCSIZE` / `WM_NCPAINT` / `WM_NCACTIVATE` / `WM_GETMINMAXINFO` to kill the residual caption strip.
- **No glass at all.** `window-vibrancy` is not a dependency; no `apply_acrylic` / `apply_mica` anywhere. Flat `--panel-bg: rgba(44,44,46,0.95)`, `--panel-border: rgba(255,255,255,0.08)`, drop shadow, solid dark GDI brush behind. The port deliberately fakes the macOS look with near-opaque grey.
- Size: tray + popover + positioning ≈ 4.5–5k lines of Rust.

---

## Three techniques worth re-implementing (not copying)

1. **Anchor direction.** Membrane's `popover_origin` (`main.rs:406`) always opens downward and clamps `y` only against the top edge. On Windows the taskbar is normally at the bottom, so the panel must open **above** the icon. Left/right/top-docked taskbars need handling too. This is a live bug, independent of any styling work, and confirms the defect identified earlier in `HUB-REDESIGN-AND-TRAY-ARCHITECTURE.md` §1.1.
2. **Blur-dismiss grace.** Membrane's `Focused(false) → hide` has no grace period. The Windows port needed ~500 ms after a tray click plus a gesture guard to avoid spurious self-dismissal. Expect the same flicker.
3. **Caption sliver.** Tauri's `decorations(false)` can leave a caption strip on Windows; `shell/dwm.rs` documents which messages to subclass if it appears.

---

## Windows glass: acrylic vs `NSVisualEffectMaterial::Popover`

What `apply_acrylic` **does** give: a genuine gaussian blur of what is behind the window, tinted by RGBA. Membrane currently passes `(18, 20, 25, 180)` — dark tint at ~70% opacity. With a 1px `rgba(255,255,255,.08)` hairline and a soft shadow it reads as glass at a glance.

What it **cannot** match:

- **No vibrancy.** The macOS `Popover` material blends material-aware colour so labels and separators shift against the backdrop. Windows acrylic is blur + flat tint; text sits unmodulated on top, so it reads flatter.
- **No noise layer.** macOS materials include fine grain that hides banding; large flat dark tints can band visibly on Windows.
- **No automatic light/dark material swap**, no active/inactive state behaviour — both palettes are yours to manage.
- **Silent degradation.** Windows drops acrylic to a solid colour when unfocused (by design in some builds), on battery saver, with "transparency effects" off, and over RDP. Assume roughly 20–30% of installs never see the blur, so a legible flat fallback is mandatory regardless.
- **Corners and shadow.** macOS gives the rounded mask, arrow and shadow free. On Windows you draw the radius in CSS and set `DWMWA_WINDOW_CORNER_PREFERENCE`; a CSS radius over acrylic can leave hard corner pixels because the backdrop applies to the HWND, not the rounded content.
- **Cost.** Acrylic re-blurs every frame behind the window, increasing GPU wake-ups for an always-resident tray app. Mica is cheaper but samples only the wallpaper — wrong material for a popover.

**Recommendation:** design the panel to be correct and attractive at flat `rgba(24,26,32,0.97)` with no blur, and treat acrylic as progressive enhancement. Glass on macOS, identical layout in flat dark on Windows. This is the same trade the Windows CodexBar port made.

---

## Caveats

- The Windows repo has several near-identical mirrors (`nesszer`, `maotai11`, `Yogitmeister`). If its logic is used as reference, pin canonical `Finesssee/Win-CodexBar` at a specific SHA. Clone inspected: `nesszer` mirror, HEAD `bdf0773f`, 2026-08-25.
- Neither app was built or run. Dismissal timings and acrylic degradation behaviour are read from source and platform knowledge, not measured on this machine.
- Clones live in scratch only (`…\scratchpad\codexbar\`). Nothing was added under `D:\Claude`.
