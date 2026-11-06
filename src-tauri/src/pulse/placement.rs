//! Window geometry shared by the Pulse and reminder panels, as plain physical-pixel numbers so it
//! tests without a display.

#[cfg(any(windows, test))]
use tauri::PhysicalSize;
#[cfg(not(windows))]
use tauri::{AppHandle, Rect};
use tauri::{LogicalSize, PhysicalPosition, WebviewWindow};

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[cfg(any(windows, test))]
impl PixelRect {
    pub fn new(position: PhysicalPosition<i32>, size: PhysicalSize<u32>) -> Self {
        let left = f64::from(position.x);
        let top = f64::from(position.y);
        Self {
            left,
            top,
            right: left + f64::from(size.width),
            bottom: top + f64::from(size.height),
        }
    }
}

pub fn physical_size((width, height): (f64, f64), scale: f64) -> (f64, f64) {
    (width * scale, height * scale)
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The edge the taskbar docks to, read from how far it shrinks the work area. The deepest of
/// several wins; no difference means auto-hide or no taskbar, so the default (bottom) is guessed.
#[cfg(any(windows, test))]
pub fn taskbar_edge(bounds: PixelRect, work: PixelRect) -> TaskbarEdge {
    let insets = [
        (TaskbarEdge::Bottom, bounds.bottom - work.bottom),
        (TaskbarEdge::Top, work.top - bounds.top),
        (TaskbarEdge::Left, work.left - bounds.left),
        (TaskbarEdge::Right, bounds.right - work.right),
    ];
    let mut best = (TaskbarEdge::Bottom, 0.0);
    for candidate in insets {
        if candidate.1 > best.1 {
            best = candidate;
        }
    }
    best.0
}

/// Where a flyout of `size` goes, like native Windows 11 flyouts: the work-area corner nearest the
/// notification area, `margin` from both edges. `icon` (the clicked icon's centre) only picks the
/// corner along the taskbar, never the distance from the edge.
#[cfg(any(windows, test))]
pub fn flyout_origin(
    bounds: PixelRect,
    work: PixelRect,
    icon: (f64, f64),
    (width, height): (f64, f64),
    margin: f64,
) -> (f64, f64) {
    let at_left = work.left + margin;
    let at_right = work.right - width - margin;
    let at_top = work.top + margin;
    let at_bottom = work.bottom - height - margin;
    let left_half = icon.0 < (bounds.left + bounds.right) / 2.0;
    let top_half = icon.1 < (bounds.top + bounds.bottom) / 2.0;
    let (x, y) = match taskbar_edge(bounds, work) {
        TaskbarEdge::Bottom => (if left_half { at_left } else { at_right }, at_bottom),
        TaskbarEdge::Top => (if left_half { at_left } else { at_right }, at_top),
        TaskbarEdge::Left => (at_left, if top_half { at_top } else { at_bottom }),
        TaskbarEdge::Right => (at_right, if top_half { at_top } else { at_bottom }),
    };
    // A panel larger than the work area keeps its top-left corner on screen.
    (x.max(at_left), y.max(at_top))
}

#[cfg(not(windows))]
const GAP: f64 = 6.0;
#[cfg(not(windows))]
const EDGE: f64 = 8.0;

#[cfg(not(windows))]
struct Area {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[cfg(not(windows))]
pub fn place_under_icon(app: &AppHandle, window: &WebviewWindow, icon: Rect, height: f64) {
    let anchor = super::anchor(app, icon);
    let (scale, area) = match app.monitor_from_point(anchor.x, anchor.top).ok().flatten() {
        Some(monitor) => {
            let area = monitor.work_area();
            let left = f64::from(area.position.x);
            let top = f64::from(area.position.y);
            (
                monitor.scale_factor(),
                Area {
                    left,
                    top,
                    right: left + f64::from(area.size.width),
                    bottom: top + f64::from(area.size.height),
                },
            )
        }
        None => (
            window.scale_factor().unwrap_or(1.0),
            Area {
                left: f64::MIN,
                top: f64::MIN,
                right: f64::MAX,
                bottom: f64::MAX,
            },
        ),
    };

    let (width, tall) = physical_size((super::WIDTH, height), scale);
    let gap = GAP * scale;
    let edge = EDGE * scale;
    let max_left = (area.right - width - edge).max(area.left + edge);
    let panel_x = (anchor.x - width / 2.0).clamp(area.left + edge, max_left);
    let below = anchor.bottom + gap;
    let panel_y = if below + tall <= area.bottom {
        below
    } else {
        (anchor.top - gap - tall).max(area.top + edge)
    };

    set_bounds(window, scale, (panel_x, panel_y), (super::WIDTH, height));
}

#[cfg(any(windows, test))]
fn scale_changes(from: f64, to: f64) -> bool {
    (from - to).abs() > 1e-6
}

/// Moves and sizes `window`. On mixed-DPI Windows the move comes first so the `WM_DPICHANGED`
/// happens before sizing, then again to undo the rectangle Windows suggests. Elsewhere resize then
/// move, as macOS keeps the bottom-left fixed on resize.
pub fn set_bounds(
    window: &WebviewWindow,
    monitor_scale: f64,
    origin: (f64, f64),
    (width, height): (f64, f64),
) {
    let position = PhysicalPosition::new(origin.0, origin.1);
    #[cfg(windows)]
    if scale_changes(
        window.scale_factor().unwrap_or(monitor_scale),
        monitor_scale,
    ) {
        let _ = window.set_position(position);
    }
    #[cfg(not(windows))]
    let _ = monitor_scale;
    let _ = window.set_size(LogicalSize::new(width, height));
    let _ = window.set_position(position);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: PixelRect = PixelRect {
        left: 0.0,
        top: 0.0,
        right: 1920.0,
        bottom: 1080.0,
    };

    fn rect(left: f64, top: f64, right: f64, bottom: f64) -> PixelRect {
        PixelRect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn taskbar_edge_follows_the_work_area_inset() {
        assert_eq!(
            taskbar_edge(BOUNDS, rect(0.0, 0.0, 1920.0, 1032.0)),
            TaskbarEdge::Bottom
        );
        assert_eq!(
            taskbar_edge(BOUNDS, rect(0.0, 48.0, 1920.0, 1080.0)),
            TaskbarEdge::Top
        );
        assert_eq!(
            taskbar_edge(BOUNDS, rect(62.0, 0.0, 1920.0, 1080.0)),
            TaskbarEdge::Left
        );
        assert_eq!(
            taskbar_edge(BOUNDS, rect(0.0, 0.0, 1858.0, 1080.0)),
            TaskbarEdge::Right
        );
    }

    #[test]
    fn taskbar_edge_defaults_to_the_bottom_without_an_inset() {
        assert_eq!(taskbar_edge(BOUNDS, BOUNDS), TaskbarEdge::Bottom);
    }

    #[test]
    fn taskbar_edge_picks_the_deepest_inset() {
        // A docked toolbar a few pixels deep must not outvote the taskbar.
        assert_eq!(
            taskbar_edge(BOUNDS, rect(0.0, 0.0, 1900.0, 1032.0)),
            TaskbarEdge::Bottom
        );
    }

    #[test]
    fn taskbar_edge_works_on_a_monitor_left_of_the_primary() {
        let bounds = rect(-1920.0, 0.0, 0.0, 1080.0);
        assert_eq!(
            taskbar_edge(bounds, rect(-1920.0, 0.0, 0.0, 1032.0)),
            TaskbarEdge::Bottom
        );
    }

    #[test]
    fn bottom_taskbar_puts_the_flyout_in_the_bottom_right_corner() {
        let area = rect(0.0, 0.0, 1920.0, 1032.0);
        // From the overflow flyout, mid-right of the screen.
        assert_eq!(
            flyout_origin(BOUNDS, area, (1760.0, 660.0), (320.0, 400.0), 12.0),
            (1588.0, 620.0)
        );
        // Pinned beside the clock gives the same corner.
        assert_eq!(
            flyout_origin(BOUNDS, area, (1700.0, 1056.0), (320.0, 400.0), 12.0),
            (1588.0, 620.0)
        );
    }

    #[test]
    fn bottom_taskbar_with_the_icon_at_the_left_uses_the_left_corner() {
        let area = rect(0.0, 0.0, 1920.0, 1032.0);
        assert_eq!(
            flyout_origin(BOUNDS, area, (200.0, 1056.0), (320.0, 400.0), 12.0),
            (12.0, 620.0)
        );
    }

    #[test]
    fn top_taskbar_puts_the_flyout_below_it() {
        let area = rect(0.0, 48.0, 1920.0, 1080.0);
        assert_eq!(
            flyout_origin(BOUNDS, area, (1800.0, 24.0), (320.0, 400.0), 12.0),
            (1588.0, 60.0)
        );
    }

    #[test]
    fn left_taskbar_puts_the_flyout_beside_it_at_the_bottom() {
        let area = rect(62.0, 0.0, 1920.0, 1080.0);
        assert_eq!(
            flyout_origin(BOUNDS, area, (31.0, 1000.0), (320.0, 400.0), 12.0),
            (74.0, 668.0)
        );
    }

    #[test]
    fn right_taskbar_puts_the_flyout_beside_it() {
        let area = rect(0.0, 0.0, 1858.0, 1080.0);
        assert_eq!(
            flyout_origin(BOUNDS, area, (1889.0, 1000.0), (320.0, 400.0), 12.0),
            (1526.0, 668.0)
        );
        // An icon in the upper half (a top-aligned tray) takes the top corner.
        assert_eq!(
            flyout_origin(BOUNDS, area, (1889.0, 100.0), (320.0, 400.0), 12.0),
            (1526.0, 12.0)
        );
    }

    #[test]
    fn flyout_stays_inside_a_secondary_monitor_with_negative_coordinates() {
        let bounds = rect(-2560.0, -200.0, 0.0, 1240.0);
        let area = rect(-2560.0, -200.0, 0.0, 1192.0);
        assert_eq!(
            flyout_origin(bounds, area, (-100.0, 400.0), (480.0, 600.0), 18.0),
            (-498.0, 574.0)
        );
    }

    #[test]
    fn flyout_taller_than_the_work_area_keeps_its_top_left_on_screen() {
        let area = rect(0.0, 0.0, 1920.0, 1032.0);
        assert_eq!(
            flyout_origin(BOUNDS, area, (1760.0, 660.0), (320.0, 2000.0), 12.0),
            (1588.0, 12.0)
        );
    }

    #[test]
    fn logical_sizes_convert_with_the_monitor_scale() {
        assert_eq!(physical_size((320.0, 400.0), 1.0), (320.0, 400.0));
        assert_eq!(physical_size((320.0, 400.0), 1.25), (400.0, 500.0));
        assert_eq!(physical_size((320.0, 400.0), 1.5), (480.0, 600.0));
        assert_eq!(physical_size((320.0, 400.0), 2.0), (640.0, 800.0));
    }

    #[test]
    fn only_a_different_scale_counts_as_a_dpi_change() {
        assert!(!scale_changes(1.25, 1.25));
        assert!(!scale_changes(1.5, 1.5 + 1e-9));
        assert!(scale_changes(1.0, 1.5));
        assert!(scale_changes(2.0, 1.25));
    }

    #[test]
    fn pixel_rect_spans_the_monitor_from_its_origin() {
        let spanned = PixelRect::new(
            PhysicalPosition::new(-1920, 30),
            PhysicalSize::new(1920, 1050),
        );
        assert_eq!(spanned, rect(-1920.0, 30.0, 0.0, 1080.0));
    }
}
