//! Renderer selection for the Slint window.
//!
//! Slint's default Windows renderer is femtovg over OpenGL. GPU-less hosts
//! (CI qualification runners, some remote sessions) have no OpenGL driver, so
//! the GPU renderer aborts with `Could not locate glCreateShader symbol`. We
//! keep the GPU renderer for real desktops and fall back to the bundled
//! software renderer (`renderer-software`, a default Slint feature) when the
//! host looks headless, when `MEMBRANE_TRAY_RENDERER=software` forces it, or
//! when the GPU renderer fails at window creation.

/// Environment override honoured for qualification and manual testing.
pub const RENDERER_ENV: &str = "MEMBRANE_TRAY_RENDERER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    /// Slint's compiled-in default (femtovg / OpenGL on Windows).
    Default,
    /// Slint's CPU software renderer.
    Software,
}

impl Renderer {
    pub fn name(self) -> &'static str {
        match self {
            Renderer::Default => "default",
            Renderer::Software => "software",
        }
    }
}

/// Pure selection logic.
///
/// * `override_env` — raw value of `MEMBRANE_TRAY_RENDERER`, if set. `software`
///   forces the software renderer; `default`/`gpu` forces the GPU renderer.
///   An explicit override always wins.
/// * `headless` — the OS reports no usable display (a GPU-less runner). Slint
///   cannot report a GPU-driver failure before a window is shown, so a headless
///   session is treated as "GPU renderer will fail".
pub fn choose_renderer(override_env: Option<&str>, headless: bool) -> Renderer {
    if let Some(value) = override_env {
        let value = value.trim();
        if value.eq_ignore_ascii_case("software") {
            return Renderer::Software;
        }
        if value.eq_ignore_ascii_case("default") || value.eq_ignore_ascii_case("gpu") {
            return Renderer::Default;
        }
    }
    if headless {
        Renderer::Software
    } else {
        Renderer::Default
    }
}

/// True when a `slint::PlatformError` looks like a missing/broken GPU driver,
/// so retrying with the software renderer is worthwhile.
pub fn looks_like_gpu_failure(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "glcreateshader",
        "opengl",
        "egl",
        "wgpu",
        "gpu",
        "graphics driver",
        "initialize opengl driver",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// Select the renderer before any window is created. Logs the choice (and the
/// reason) to stderr, which qualification captures. Returns the renderer that
/// is actually active.
pub fn install() -> Renderer {
    let override_env = std::env::var(RENDERER_ENV).ok();
    let headless = is_headless_session();
    let choice = choose_renderer(override_env.as_deref(), headless);
    let reason = if override_env
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        "env-override"
    } else if headless {
        "headless-session"
    } else {
        "default"
    };

    match choice {
        Renderer::Software => activate_software(reason),
        Renderer::Default => {
            log_renderer(Renderer::Default, reason);
            Renderer::Default
        }
    }
}

/// Runtime fallback: the GPU renderer failed while creating the window. Switch
/// to the software renderer so a second attempt can succeed.
pub fn force_software() -> Renderer {
    activate_software("gpu-renderer-failure")
}

fn activate_software(reason: &str) -> Renderer {
    match slint::BackendSelector::new()
        .renderer_name("software".to_string())
        .select()
    {
        Ok(()) => {
            log_renderer(Renderer::Software, reason);
            Renderer::Software
        }
        Err(error) => {
            eprintln!(
                "membrane-tray: software renderer unavailable ({error}); continuing with default renderer"
            );
            Renderer::Default
        }
    }
}

fn log_renderer(renderer: Renderer, reason: &str) {
    eprintln!(
        "membrane-tray: renderer={} reason={reason}",
        renderer.name()
    );
}

#[cfg(windows)]
fn is_headless_session() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
    };
    // A session with no attached display surface reports a zero-size screen;
    // that is the GPU-less runner case where the OpenGL renderer cannot start.
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    width <= 0 || height <= 0
}

#[cfg(not(windows))]
fn is_headless_session() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_software_wins_over_gpu_desktop() {
        assert_eq!(
            choose_renderer(Some("software"), false),
            Renderer::Software
        );
        assert_eq!(
            choose_renderer(Some(" SOFTWARE \n"), false),
            Renderer::Software
        );
    }

    #[test]
    fn env_override_default_wins_over_headless() {
        assert_eq!(choose_renderer(Some("default"), true), Renderer::Default);
        assert_eq!(choose_renderer(Some("gpu"), true), Renderer::Default);
    }

    #[test]
    fn headless_session_selects_software_without_override() {
        assert_eq!(choose_renderer(None, true), Renderer::Software);
    }

    #[test]
    fn gpu_desktop_keeps_default_renderer() {
        assert_eq!(choose_renderer(None, false), Renderer::Default);
        assert_eq!(choose_renderer(Some("weird-value"), false), Renderer::Default);
    }

    #[test]
    fn gpu_failure_messages_are_recognized() {
        assert!(looks_like_gpu_failure(
            "Failed to initialize OpenGL driver: Could not locate glCreateShader symbol"
        ));
        assert!(!looks_like_gpu_failure("window title bar missing"));
    }
}
