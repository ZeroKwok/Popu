use crate::protocol::{Position, Request};

pub const NOTIFICATION_STACK_GAP: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlacement {
    /// The requested content size in logical pixels.
    pub logical_size: (u32, u32),
    /// The native window size in physical pixels.
    pub size: (u32, u32),
    pub position: Point,
}

pub fn select_monitor(monitors: &[Rect], cursor: Point) -> Option<Rect> {
    monitors
        .iter()
        .copied()
        .find(|monitor| contains(*monitor, cursor))
        .or_else(|| monitors.first().copied())
}

/// Resolves the final clamped window size and placement from one physical
/// display area. Request dimensions and margins are logical pixels; native
/// coordinates remain physical pixels.
pub fn resolve(
    request: &Request,
    area: Rect,
    scale_factor: f64,
    logical_margin: u32,
) -> WindowPlacement {
    let scale_factor = scale_factor.max(f64::MIN_POSITIVE);
    let logical_area = (
        ((area.width as f64 / scale_factor).round() as u32).max(1),
        ((area.height as f64 / scale_factor).round() as u32).max(1),
    );
    let logical_size = request.window_size(logical_area);
    let size = (
        ((logical_size.0 as f64 * scale_factor).round() as u32).min(area.width),
        ((logical_size.1 as f64 * scale_factor).round() as u32).min(area.height),
    );
    let physical_margin = (logical_margin as f64 * scale_factor).round() as u32;
    let margin_x = physical_margin.min(area.width.saturating_sub(size.0));
    let margin_y = physical_margin.min(area.height.saturating_sub(size.1));
    let position = match request.position {
        Position::BottomRight => Point {
            x: area.x + (area.width - size.0 - margin_x) as i32,
            y: area.y + (area.height - size.1 - margin_y) as i32,
        },
        Position::CursorScreenCenter => Point {
            x: area.x + (area.width.saturating_sub(size.0) / 2) as i32,
            y: area.y + (area.height.saturating_sub(size.1) / 2) as i32,
        },
    };
    WindowPlacement {
        logical_size,
        size,
        position,
    }
}

/// Returns the vertical distance needed to place a notification above all of
/// its older visible siblings. Heights are physical pixels because native
/// window coordinates are physical.
pub fn notification_stack_offset(preceding_heights: impl IntoIterator<Item = u32>) -> i32 {
    preceding_heights.into_iter().fold(0_i32, |offset, height| {
        offset
            .saturating_add(i32::try_from(height).unwrap_or(i32::MAX))
            .saturating_add(NOTIFICATION_STACK_GAP as i32)
    })
}

fn contains(rect: Rect, point: Point) -> bool {
    let right = i64::from(rect.x) + i64::from(rect.width);
    let bottom = i64::from(rect.y) + i64::from(rect.height);
    i64::from(point.x) >= i64::from(rect.x)
        && i64::from(point.x) < right
        && i64::from(point.y) >= i64::from(rect.y)
        && i64::from(point.y) < bottom
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Request;

    fn request(window: &str) -> Request {
        Request::parse(&format!(
            r#"{{"version":"1.0","mode":"dialog","window":{window},"content":{{"title":"Test"}},"actions":[{{"id":"ok","label":"OK","submit":true}}]}}"#
        ))
        .unwrap()
    }

    #[test]
    fn selects_monitor_with_negative_coordinates() {
        let monitors = [
            Rect {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
        ];
        assert_eq!(
            select_monitor(&monitors, Point { x: -100, y: 10 }),
            Some(monitors[0])
        );
    }

    #[test]
    fn percentage_dimensions_fill_the_selected_area() {
        let placement = resolve(
            &request(r#"{"width":"100%","height":"100%"}"#),
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
            1.0,
            16,
        );
        assert_eq!(placement.size, (1920, 1040));
        assert_eq!(placement.position, Point { x: 0, y: 0 });
    }

    #[test]
    fn oversized_pixel_dimensions_are_clamped_independently() {
        let placement = resolve(
            &request(r#"{"width":2000,"height":300}"#),
            Rect {
                x: -1280,
                y: 0,
                width: 1280,
                height: 720,
            },
            1.0,
            16,
        );
        assert_eq!(placement.size, (1280, 300));
        assert_eq!(placement.position, Point { x: -1280, y: 210 });
    }

    #[test]
    fn logical_notification_size_scales_without_changing_its_layout_size() {
        let request = Request::parse(
            r#"{"version":"1.0","mode":"notify","position":"bottom_right","content":{"title":"Test"}}"#,
        )
        .unwrap();
        let placement = resolve(
            &request,
            Rect {
                x: 0,
                y: 0,
                width: 3840,
                height: 2160,
            },
            2.0,
            16,
        );
        assert_eq!(placement.logical_size, (360, 180));
        assert_eq!(placement.size, (720, 360));
        assert_eq!(placement.position, Point { x: 3088, y: 1768 });
    }

    #[test]
    fn notification_stack_offset_accounts_for_each_preceding_height() {
        let bottom = 1_024_i32;
        let heights = [620_u32, 180, 180];
        let positions: Vec<_> = heights
            .iter()
            .enumerate()
            .map(|(index, height)| {
                bottom - *height as i32 - notification_stack_offset(heights[..index].iter().copied())
            })
            .collect();

        assert_eq!(positions, [404, 212, 20]);
        for (positions, heights) in positions.windows(2).zip(heights.windows(2)) {
            assert_eq!(
                positions[1] + heights[1] as i32 + NOTIFICATION_STACK_GAP as i32,
                positions[0]
            );
        }
    }
}
