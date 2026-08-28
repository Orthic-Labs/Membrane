//! Native Windows notification-area icon.
//!
//! Status glyphs are shapes first so a monochrome shell still distinguishes
//! running (filled), transitional (half), and unavailable (hollow) states.
//! Raster size follows current system DPI: 16, 20, 24, or 32 px.

use crate::supervisor::State;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Starting,
    Stopping,
    Offline,
    Restarting,
    CrashLoop,
}

impl Status {
    pub const fn from_state(state: State) -> Self {
        match state {
            State::Running => Self::Running,
            State::Starting => Self::Starting,
            State::Draining => Self::Stopping,
            State::Stopped => Self::Offline,
            State::Backoff => Self::Restarting,
            State::CrashLoop => Self::CrashLoop,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Starting => "Starting",
            Self::Stopping => "Stopping",
            Self::Offline => "Offline",
            Self::Restarting => "Restarting",
            Self::CrashLoop => "Crash loop",
        }
    }

    const fn tint(self) -> [u8; 4] {
        match self {
            Self::Running => [63, 217, 139, 255],
            Self::Starting | Self::Stopping => [240, 178, 60, 255],
            Self::Offline | Self::Restarting | Self::CrashLoop => [255, 107, 115, 255],
        }
    }

    const fn kind(self) -> GlyphKind {
        match self {
            Self::Running => GlyphKind::Filled,
            Self::Starting | Self::Stopping => GlyphKind::Half,
            Self::Offline | Self::Restarting | Self::CrashLoop => GlyphKind::Hollow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlyphKind {
    Filled,
    Half,
    Hollow,
    Dash,
}

pub fn icon_size_for_scale(scale: f64) -> u32 {
    if scale >= 1.75 {
        32
    } else if scale >= 1.25 {
        24
    } else if scale >= 1.05 {
        20
    } else {
        16
    }
}

pub fn status_icon(status: Status, size: u32) -> Icon {
    let size = size.clamp(16, 32);
    let mut pixels = vec![0_u8; (size * size * 4) as usize];
    let tint = status.tint();
    let kind = status.kind();
    let margin = (size / 5).max(2);
    let left = margin;
    let top = margin;
    let right = size.saturating_sub(margin + 1);
    let bottom = right;
    let put = |pixels: &mut [u8], x: u32, y: u32| {
        let index = ((y * size + x) * 4) as usize;
        pixels[index..index + 4].copy_from_slice(&tint);
    };
    for y in top..=bottom {
        for x in left..=right {
            let border = x == left || x == right || y == top || y == bottom;
            let filled = match kind {
                GlyphKind::Filled => true,
                GlyphKind::Half => x <= left + (right - left) / 2,
                GlyphKind::Hollow | GlyphKind::Dash => false,
            };
            if filled || (kind == GlyphKind::Hollow && border) {
                put(&mut pixels, x, y);
            }
        }
    }
    if kind == GlyphKind::Dash {
        let y = top + (bottom - top) / 2;
        for x in left..=right {
            put(&mut pixels, x, y);
        }
    }
    Icon::from_rgba(pixels, size, size).expect("status icon dimensions are valid")
}

pub fn create_tray(status: Status) -> tray_icon::Result<TrayIcon> {
    TrayIconBuilder::new()
        .with_tooltip(format!("Membrane — {}", status.label()))
        .with_icon(status_icon(status, icon_size_for_scale(current_scale())))
        .with_menu_on_left_click(false)
        .build()
}

pub fn update_tray(tray: &TrayIcon, status: Status, reason: &str) -> tray_icon::Result<()> {
    let size = icon_size_for_scale(current_scale_for_tray(tray));
    tray.set_icon(Some(status_icon(status, size)))?;
    tray.set_tooltip(Some(format!("Membrane — {} · {}", status.label(), reason)))
}

fn current_scale() -> f64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGPIXELSX};
        let dc = unsafe { GetDC(std::ptr::null_mut()) };
        if !dc.is_null() {
            let dpi = unsafe { GetDeviceCaps(dc, LOGPIXELSX as i32) };
            unsafe { ReleaseDC(std::ptr::null_mut(), dc) };
            if dpi > 0 {
                return f64::from(dpi) / 96.0;
            }
        }
    }
    1.0
}

fn current_scale_for_tray(tray: &TrayIcon) -> f64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGPIXELSX};
        let hwnd = tray.window_handle();
        let dc = unsafe { GetDC(hwnd) };
        if !dc.is_null() {
            let dpi = unsafe { GetDeviceCaps(dc, LOGPIXELSX as i32) };
            unsafe { ReleaseDC(hwnd, dc) };
            if dpi > 0 {
                return f64::from(dpi) / 96.0;
            }
        }
    }
    current_scale()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_sizes_are_shell_native() {
        assert_eq!(icon_size_for_scale(1.0), 16);
        assert_eq!(icon_size_for_scale(1.25), 24);
        assert_eq!(icon_size_for_scale(1.5), 24);
        assert_eq!(icon_size_for_scale(2.0), 32);
    }

    #[test]
    fn every_status_has_shape_and_dimensions() {
        for status in [
            Status::Running,
            Status::Starting,
            Status::Stopping,
            Status::Offline,
            Status::Restarting,
            Status::CrashLoop,
        ] {
            let _ = status_icon(status, 16);
            let _ = status_icon(status, 20);
            let _ = status_icon(status, 24);
            let _ = status_icon(status, 32);
        }
    }
}
