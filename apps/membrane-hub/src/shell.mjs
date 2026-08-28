// Membrane Hub window shell — chrome contract ported from HeardRight's
// tauri-app-next shell (src/lib/ui.ts grid, TitleBar.tsx / Sidebar.tsx / WindowControls.tsx
// per-OS chrome split, hooks/usePlatform.ts). Shell geometry is preserved from
// HeardRight: 208px sidebar, 40px full-width titlebar, fused active tab, and a
// one-column narrow layout. Membrane owns content, data, and verdict language.
//
// macOS keeps native decorations + Overlay titlebar style with a traffic-light
// gutter reserved in chrome; Windows renders custom caption controls in the
// webview. This module only renders/wires the in-webview half of that split —
// the native decoration mode itself is set in src-tauri (out of scope for this
// port; see docs/design/hub-redesign/HUB-REDESIGN-AND-TRAY-ARCHITECTURE.md §3.2).

export async function detectPlatform() {
  try {
    const { platform } = await import("../vendor/@tauri-apps/plugin-os/index.js");
    const value = await platform();
    return value === "macos" ? "macos" : value === "windows" ? "windows" : value || "unknown";
  } catch {
    return "unknown";
  }
}

async function currentWindow() {
  const { getCurrentWindow } = await import("../vendor/@tauri-apps/api/window.js");
  return getCurrentWindow();
}

export function wireCaptionControls(root) {
  const minimize = root.querySelector("[data-caption='minimize']");
  const maximize = root.querySelector("[data-caption='maximize']");
  const close = root.querySelector("[data-caption='close']");
  minimize?.addEventListener("click", async () => { try { (await currentWindow()).minimize(); } catch {} });
  maximize?.addEventListener("click", async () => { try { (await currentWindow()).toggleMaximize(); } catch {} });
  close?.addEventListener("click", async () => { try { (await currentWindow()).close(); } catch {} });
}

// Roving-tabindex keyboard nav for the sidebar rail, ported from HeardRight's
// Sidebar.tsx arrow-key handling: only the active item is tab-stoppable,
// arrow keys move focus and selection within the list.
export function wireRovingSidebar(nav, { onActivate } = {}) {
  if (!nav) return;
  const items = () => Array.from(nav.querySelectorAll("a"));
  const sync = () => {
    for (const item of items()) item.tabIndex = item.classList.contains("active") || item.getAttribute("aria-current") === "page" ? 0 : -1;
  };
  sync();
  nav.addEventListener("keydown", (event) => {
    const list = items();
    const index = list.indexOf(document.activeElement);
    if (index === -1) return;
    let next = -1;
    if (event.key === "ArrowDown" || event.key === "ArrowRight") next = (index + 1) % list.length;
    else if (event.key === "ArrowUp" || event.key === "ArrowLeft") next = (index - 1 + list.length) % list.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = list.length - 1;
    if (next === -1) return;
    event.preventDefault();
    list[next].focus();
    onActivate?.(list[next]);
  });
  return sync;
}

export async function applyShellChrome(root) {
  const platform = await detectPlatform();
  root.dataset.platform = platform;
  wireCaptionControls(root);
  return platform;
}
