use crate::protocol::Position;

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

pub fn select_monitor(monitors: &[Rect], cursor: Point) -> Option<Rect> {
    monitors
        .iter()
        .copied()
        .find(|monitor| contains(*monitor, cursor))
        .or_else(|| monitors.first().copied())
}

pub fn calculate(position: Position, work_area: Rect, window: (u32, u32), margin: u32) -> Point {
    let width = window.0.min(work_area.width);
    let height = window.1.min(work_area.height);
    let margin_x = margin.min(work_area.width.saturating_sub(width));
    let margin_y = margin.min(work_area.height.saturating_sub(height));
    match position {
        Position::BottomRight => Point {
            x: work_area.x + (work_area.width - width - margin_x) as i32,
            y: work_area.y + (work_area.height - height - margin_y) as i32,
        },
        Position::CursorScreenCenter => Point {
            x: work_area.x + (work_area.width.saturating_sub(width) / 2) as i32,
            y: work_area.y + (work_area.height.saturating_sub(height) / 2) as i32,
        },
    }
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
    fn falls_back_to_primary_monitor() {
        let primary = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        assert_eq!(
            select_monitor(&[primary], Point { x: 4000, y: 4000 }),
            Some(primary)
        );
    }

    #[test]
    fn positions_at_bottom_right_inside_work_area() {
        let point = calculate(
            Position::BottomRight,
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
            (360, 180),
            16,
        );
        assert_eq!(point, Point { x: 1544, y: 844 });
    }

    #[test]
    fn centers_and_clamps_oversized_window() {
        let point = calculate(
            Position::CursorScreenCenter,
            Rect {
                x: -1280,
                y: 0,
                width: 1280,
                height: 720,
            },
            (2000, 900),
            16,
        );
        assert_eq!(point, Point { x: -1280, y: 0 });
    }
}
