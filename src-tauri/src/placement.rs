use crate::protocol::{Position, Request};

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

/// Resolves the final clamped window size and placement from one display area.
pub fn resolve(request: &Request, area: Rect, margin: u32) -> WindowPlacement {
    let size = request.window_size((area.width, area.height));
    let margin_x = margin.min(area.width.saturating_sub(size.0));
    let margin_y = margin.min(area.height.saturating_sub(size.1));
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
    WindowPlacement { size, position }
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
            16,
        );
        assert_eq!(placement.size, (1280, 300));
        assert_eq!(placement.position, Point { x: -1280, y: 210 });
    }
}
