#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Determine which edge owns a notification-area rectangle. The tray icon
/// normally sits outside the monitor work area because taskbar consumes that
/// strip; comparing against `rcWork` handles all four taskbar orientations.
pub fn edge_for(anchor: Rect, work: Rect) -> TaskbarEdge {
    let distances = [
        (TaskbarEdge::Top, (anchor.bottom - work.top).unsigned_abs()),
        (
            TaskbarEdge::Bottom,
            (anchor.top - work.bottom).unsigned_abs(),
        ),
        (TaskbarEdge::Left, (anchor.right - work.left).unsigned_abs()),
        (
            TaskbarEdge::Right,
            (anchor.left - work.right).unsigned_abs(),
        ),
    ];
    distances
        .into_iter()
        .min_by_key(|(_, distance)| *distance)
        .map(|(edge, _)| edge)
        .unwrap_or(TaskbarEdge::Bottom)
}

pub fn origin(anchor: Rect, popover: Size, work: Rect, edge: TaskbarEdge) -> Point {
    let clamp_x = |x: i32| x.max(work.left).min(work.right - popover.width);
    let clamp_y = |y: i32| y.max(work.top).min(work.bottom - popover.height);
    match edge {
        TaskbarEdge::Top => Point {
            x: clamp_x(anchor.left + (anchor.right - anchor.left - popover.width) / 2),
            y: clamp_y(anchor.bottom),
        },
        TaskbarEdge::Bottom => {
            let x = clamp_x(anchor.left + (anchor.right - anchor.left - popover.width) / 2);
            let below = anchor.bottom;
            let above = anchor.top - popover.height;
            let y = if below + popover.height <= work.bottom {
                below
            } else if above >= work.top {
                above
            } else if work.bottom - below >= anchor.top - work.top {
                work.bottom - popover.height
            } else {
                work.top
            };
            Point { x, y: clamp_y(y) }
        }
        TaskbarEdge::Left => Point {
            x: clamp_x(anchor.right),
            y: clamp_y(anchor.top + (anchor.bottom - anchor.top - popover.height) / 2),
        },
        TaskbarEdge::Right => Point {
            x: clamp_x(anchor.left - popover.width),
            y: clamp_y(anchor.top + (anchor.bottom - anchor.top - popover.height) / 2),
        },
    }
}

#[cfg(windows)]
pub fn monitor_work_area(anchor: Rect) -> Option<Rect> {
    use windows_sys::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    };
    let point = POINT {
        x: ((anchor.left + anchor.right) / 2),
        y: ((anchor.top + anchor.bottom) / 2),
    };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    Some(Rect {
        left: info.rcWork.left,
        top: info.rcWork.top,
        right: info.rcWork.right,
        bottom: info.rcWork.bottom,
    })
}

#[cfg(not(windows))]
pub fn monitor_work_area(_anchor: Rect) -> Option<Rect> {
    None
}

#[derive(Debug, Default)]
pub struct DismissGuard {
    clicked_at_ms: Option<u64>,
    gesture: bool,
}
impl DismissGuard {
    pub fn tray_click(&mut self, now_ms: u64) {
        self.clicked_at_ms = Some(now_ms);
        self.gesture = false;
    }
    pub fn pointer_down(&mut self) {
        self.gesture = true;
    }
    pub fn pointer_up_or_cancel(&mut self) {
        self.gesture = false;
    }
    pub fn should_dismiss(&self, now_ms: u64, focus_lost: bool) -> bool {
        if !focus_lost || self.gesture {
            return false;
        }
        self.clicked_at_ms
            .map_or(true, |at| now_ms.saturating_sub(at) >= 500)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const WORK: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };
    const PANEL: Size = Size {
        width: 300,
        height: 500,
    };
    #[test]
    fn bottom_taskbar_opens_above() {
        let point = origin(
            Rect {
                left: 900,
                top: 980,
                right: 920,
                bottom: 1000,
            },
            PANEL,
            WORK,
            TaskbarEdge::Bottom,
        );
        assert_eq!(point.x, 760);
        assert_eq!(point.y, 480);
    }
    #[test]
    fn top_taskbar_opens_below() {
        assert_eq!(
            origin(
                Rect {
                    left: 900,
                    top: 20,
                    right: 920,
                    bottom: 40
                },
                PANEL,
                WORK,
                TaskbarEdge::Top
            )
            .y,
            40
        );
    }
    #[test]
    fn side_taskbars_stay_in_work_area() {
        assert_eq!(
            origin(
                Rect {
                    left: 0,
                    top: 500,
                    right: 20,
                    bottom: 520
                },
                PANEL,
                WORK,
                TaskbarEdge::Left
            )
            .x,
            20
        );
        assert_eq!(
            origin(
                Rect {
                    left: 1900,
                    top: 500,
                    right: 1920,
                    bottom: 520
                },
                PANEL,
                WORK,
                TaskbarEdge::Right
            )
            .x,
            1600
        );
    }
    #[test]
    fn blur_grace_and_gesture_guard() {
        let mut g = DismissGuard::default();
        g.tray_click(1_000);
        assert!(!g.should_dismiss(1_499, true));
        assert!(g.should_dismiss(1_500, true));
        g.tray_click(2_000);
        g.pointer_down();
        assert!(!g.should_dismiss(3_000, true));
        g.pointer_up_or_cancel();
        assert!(g.should_dismiss(3_000, true));
    }
}
