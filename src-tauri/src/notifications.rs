//! Geometry for one notification stack. No window handles or mutable scheduler state.

use crate::placement::{Display, Point, WindowPlacement};
use crate::protocol::{Position, Request};

pub struct Layout {
    pub display: Display,
}

impl Layout {
    pub fn new(display: Display) -> Self {
        Self { display }
    }

    pub fn notification_size(self, request: &Request) -> WindowPlacement {
        let mut request = request.clone();
        request.position = Position::BottomRight;
        self.display.resolve(&request)
    }

    /// All sizes and coordinates are physical; spacing is converted once per plan.
    /// The first item may fill the work area. Subsequent items must fit in full.
    pub fn stack(&self, sizes: &[(u32, u32)]) -> Option<Vec<Point>> {
        let gap = (12.0 * self.display.scale).round() as i64;
        let margin = (16.0 * self.display.scale).round() as i64;
        let mut next_bottom = i64::from(self.display.area.y) + i64::from(self.display.area.height);
        let mut positions = Vec::with_capacity(sizes.len());
        for (index, &(width, height)) in sizes.iter().enumerate() {
            if width == 0
                || height == 0
                || width > self.display.area.width
                || height > self.display.area.height
            {
                return None;
            }
            if index == 0 {
                next_bottom -= margin.min(i64::from(self.display.area.height - height));
            }
            let top = next_bottom - i64::from(height);
            if top < i64::from(self.display.area.y) {
                return None;
            }
            let right_margin = margin.min(i64::from(self.display.area.width - width));
            positions.push(Point {
                x: (i64::from(self.display.area.x) + i64::from(self.display.area.width - width)
                    - right_margin) as i32,
                y: top as i32,
            });
            next_bottom = top - gap;
        }
        Some(positions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::Rect;

    fn display(scale: f64) -> Layout {
        Layout::new(Display {
            area: Rect {
                x: -2560,
                y: -100,
                width: 2560,
                height: 1400,
            },
            scale,
        })
    }

    #[test]
    fn compaction_uses_actual_sizes_and_one_bottom_anchor() {
        let d = display(1.0);
        let p = d.stack(&[(970, 500), (1080, 560), (360, 200)]).unwrap();
        assert_eq!(p.iter().map(|p| p.y).collect::<Vec<_>>(), [784, 212, 0]);
        let compacted = d.stack(&[(1080, 560), (360, 200)]).unwrap();
        assert_eq!(compacted[0].y, 724);
        assert_eq!(compacted[1].y, 512);
        assert!(d.stack(&[(1080, 560), (970, 900)]).is_none());
    }

    #[test]
    fn spacing_scales_with_dpi_and_a_single_large_window_can_fill_the_area() {
        let d = display(2.0);
        let p = d.stack(&[(720, 360), (800, 400)]).unwrap();
        assert_eq!(p[0].y - p[1].y - 400, 24);
        assert_eq!(p[0].x, -752);
        assert_eq!(
            d.stack(&[(2560, 1400)]).unwrap()[0],
            Point { x: -2560, y: -100 }
        );
        assert!(d.stack(&[(2560, 1400), (100, 100)]).is_none());
    }
}
