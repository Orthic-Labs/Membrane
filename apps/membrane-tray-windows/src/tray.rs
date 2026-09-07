//! Native Windows notification-area icon.
//!
//! Embed approved Membrane artwork at shell-native DPI sizes.
//! Lifecycle state remains visible in a small badge, tooltip & popover verdicts.
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

fn artwork(size: u32) -> image::RgbaImage {
    let png: &[u8] = match size {
        16 => include_bytes!("../../membrane-hub/assets/Membrane-Icon-Pack/Windows/PNG/Membrane-16x16.png"),
        20 => include_bytes!("../../membrane-hub/assets/Membrane-Icon-Pack/Windows/PNG/Membrane-20x20.png"),
        24 => include_bytes!("../../membrane-hub/assets/Membrane-Icon-Pack/Windows/PNG/Membrane-24x24.png"),
        _ => include_bytes!("../../membrane-hub/assets/Membrane-Icon-Pack/Windows/PNG/Membrane-32x32.png"),
    };
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .expect("embedded Membrane icon pack must decode")
        .to_rgba8()
}

fn badged_artwork(status: Status, size: u32) -> image::RgbaImage {
    let mut pixels = artwork(size);
    let size = pixels.width();
    let radius = (size / 8) as i32;
    let center = size as i32 - radius - 1;
    for y in center - radius..=center + radius {
        for x in center - radius..=center + radius {
            if (x - center).pow(2) + (y - center).pow(2) <= radius.pow(2) {
                pixels.put_pixel(x as u32, y as u32, image::Rgba(status.tint()));
            }
        }
    }
    pixels
}

pub fn app_icon(status: Status, size: u32) -> Icon {
    let pixels = badged_artwork(status, size);
    let (width, height) = pixels.dimensions();
    Icon::from_rgba(pixels.into_raw(), width, height).expect("Membrane icon dimensions are valid")
}

pub fn create_tray(status: Status) -> tray_icon::Result<TrayIcon> {
    TrayIconBuilder::new()
        .with_tooltip(format!("Membrane — {}", status.label()))
        .with_icon(app_icon(status, icon_size_for_scale(current_scale())))
        .with_menu_on_left_click(false)
        .build()
}

pub fn update_tray(tray: &TrayIcon, status: Status, reason: &str) -> tray_icon::Result<()> {
    let size = icon_size_for_scale(current_scale_for_tray(tray));
    tray.set_icon(Some(app_icon(status, size)))?;
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
    fn approved_artwork_preserves_shape_color_and_native_dimensions() {
        for size in [16, 20, 24, 32] {
            let pixels = artwork(size);
            assert_eq!(pixels.dimensions(), (size, size));
            assert!(pixels.pixels().any(|pixel| pixel[3] == 0), "transparent silhouette required");
            assert!(pixels.pixels().any(|pixel| pixel[3] > 0 && pixel[2] > pixel[1]),
                "approved purple artwork must not become a green status square");
            for status in [Status::Running, Status::Starting, Status::Stopping,
                Status::Offline, Status::Restarting, Status::CrashLoop] {
                let badged = badged_artwork(status, size);
                let center = size - size / 8 - 1;
                assert_eq!(badged.get_pixel(center, center).0, status.tint());
                for (x, y, pixel) in pixels.enumerate_pixels() {
                    if x < size - size / 4 - 1 || y < size - size / 4 - 1 {
                        assert_eq!(badged.get_pixel(x, y), pixel, "status must preserve main artwork");
                    }
                }
                let _ = app_icon(status, size);
            }
        }
    }
}
