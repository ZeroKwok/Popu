use crate::config::{Config, DEFAULT_EDGE_MARGIN_PX};
use crate::protocol::{Mode, Position, Request, WindowDimension};
use serde::Deserialize;

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
    /// The physical display area selected for this window.
    pub area: Rect,
    /// The requested content size in logical pixels.
    pub logical_size: (u32, u32),
    /// The native window size in physical pixels.
    pub size: (u32, u32),
    pub position: Point,
}

/// One immutable physical display/DPI snapshot for a complete layout pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    pub area: Rect,
    pub scale: f64,
}

impl Display {
    pub fn resolve(self, request: &Request) -> WindowPlacement {
        resolve(request, self.area, self.scale, DEFAULT_EDGE_MARGIN_PX)
    }
}

/// A renderer measurement in CSS/logical pixels, never native coordinates.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ContentSize {
    pub width: f64,
    pub height: f64,
}

/// Runtime geometry, separate from the caller's immutable request. Content may
/// change the preferred height until first presentation; afterwards only display
/// constraints can affect the actual size. Parking/revealing keeps this state.
pub struct WindowLayout {
    display: Display,
    position: Position,
    width: WindowDimension,
    height: Option<WindowDimension>,
    fallback: u32,
    min_height: u32,
    max_height: u32,
    margin: u32,
    preferred_height: Option<u32>,
    locked: bool,
}

impl WindowLayout {
    pub fn new(request: &Request, display: Display) -> Self {
        Self::with_config(request, display, &Config::default())
    }

    pub fn with_config(request: &Request, display: Display, config: &Config) -> Self {
        let (defaults, margin) = match request.mode {
            Mode::Dialog => (config.dialog.window, DEFAULT_EDGE_MARGIN_PX),
            Mode::Notify => (config.notification.window, config.notification.edge_margin_px),
        };
        Self {
            display,
            position: if request.mode == Mode::Notify {
                Position::BottomRight
            } else {
                request.position
            },
            width: request
                .window
                .width
                .clone()
                .unwrap_or(WindowDimension::Pixels(f64::from(defaults.default_width_px))),
            height: request.window.height.clone(),
            fallback: defaults.fallback_height_px,
            min_height: defaults.auto_min_height_px,
            max_height: defaults.auto_max_height_px,
            margin,
            preferred_height: None,
            locked: false,
        }
    }

    fn bounds(&self) -> (u32, u32) {
        logical_area(self.display.area, self.display.scale)
    }

    pub fn placement(&self) -> WindowPlacement {
        let bounds = self.bounds();
        let height = if let Some(height) = &self.height {
            height.resolve(bounds.1)
        } else {
            let available = bounds.1.saturating_sub(self.margin.saturating_mul(2)).max(1);
            let max = self.max_height.min(available);
            self.preferred_height
                .unwrap_or(self.fallback)
                .clamp(self.min_height.min(max), max)
        };
        resolve_size(
            self.position,
            (self.width.resolve(bounds.0), height),
            self.display.area,
            self.display.scale,
            self.margin,
        )
    }

    /// False asks the renderer to retry at the current width. Missing/invalid
    /// measurements retain a bounded fallback, including for legacy templates.
    pub fn measure(&mut self, measurement: Option<ContentSize>) -> bool {
        if !self.accepts_content_size() {
            return true;
        }
        if let Some(size) = measurement {
            let width = f64::from(self.placement().logical_size.0);
            if !size.width.is_finite() || (size.width - width).abs() > 1.0 {
                return false;
            }
            self.preferred_height = (size.height.is_finite() && size.height > 0.0)
                .then(|| size.height.ceil().min(f64::from(u32::MAX)) as u32);
        } else {
            self.preferred_height = None;
        }
        true
    }

    pub fn accepts_content_size(&self) -> bool {
        !self.locked && self.height.is_none()
    }

    pub fn lock(&mut self) {
        // Store the resolved preference, rather than unbounded content height,
        // so changing screens cannot grow a previously shown window unexpectedly.
        if self.height.is_none() && !self.locked {
            self.preferred_height = Some(self.placement().logical_size.1);
        }
        self.locked = true;
    }

    pub fn set_display(&mut self, display: Display) {
        self.display = display;
    }
}

fn logical_area(area: Rect, scale: f64) -> (u32, u32) {
    let scale = scale.max(f64::MIN_POSITIVE);
    (
        ((area.width as f64 / scale).round() as u32).max(1),
        ((area.height as f64 / scale).round() as u32).max(1),
    )
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
    resolve_size(
        request.position,
        request.window_size(logical_area(area, scale_factor)),
        area,
        scale_factor,
        logical_margin,
    )
}

fn resolve_size(
    position: Position,
    logical_size: (u32, u32),
    area: Rect,
    scale_factor: f64,
    logical_margin: u32,
) -> WindowPlacement {
    let scale_factor = scale_factor.max(f64::MIN_POSITIVE);
    let size = (
        ((logical_size.0 as f64 * scale_factor).round() as u32).min(area.width),
        ((logical_size.1 as f64 * scale_factor).round() as u32).min(area.height),
    );
    let physical_margin = (logical_margin as f64 * scale_factor).round() as u32;
    let margin_x = physical_margin.min(area.width.saturating_sub(size.0));
    let margin_y = physical_margin.min(area.height.saturating_sub(size.1));
    let position = match position {
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
        area,
        logical_size,
        size,
        position,
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
        assert_eq!(placement.logical_size, (360, 220));
        assert_eq!(placement.size, (720, 440));
        assert_eq!(placement.position, Point { x: 3088, y: 1688 });
    }

    fn layout(window: &str) -> WindowLayout {
        WindowLayout::new(
            &request(window),
            Display {
                area: Rect {
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1040,
                },
                scale: 1.0,
            },
        )
    }

    #[test]
    fn content_height_uses_limits_and_can_shrink_before_first_show() {
        let mut layout = layout(r#"{"width":500}"#);
        for (natural, expected) in [(40.0, 160), (410.2, 411), (5000.0, 720), (200.0, 200)] {
            assert!(layout.measure(Some(ContentSize {
                width: 500.0,
                height: natural
            })));
            assert_eq!(layout.placement().logical_size, (500, expected));
        }
    }

    #[test]
    fn notification_window_uses_configured_width_heights_and_margin() {
        let request = Request::parse(r#"{"version":"1.0","mode":"notify","content":{"title":"Test"}}"#).unwrap();
        let display = Display {
            area: Rect { x: 0, y: 0, width: 1200, height: 450 },
            scale: 1.5,
        };
        let mut config = Config::default();
        config.notification.edge_margin_px = 20;
        config.notification.window = crate::config::WindowDefaults {
            default_width_px: 400,
            auto_min_height_px: 140,
            auto_max_height_px: 350,
            fallback_height_px: 180,
        };
        let mut layout = WindowLayout::with_config(&request, display, &config);
        assert_eq!(layout.placement().logical_size, (400, 180));
        assert_eq!(layout.placement().position, Point { x: 570, y: 150 });

        for (natural, expected) in [(40.0, 140), (600.0, 260)] {
            assert!(layout.measure(Some(ContentSize { width: 400.0, height: natural })));
            assert_eq!(layout.placement().logical_size.1, expected);
        }
    }

    #[test]
    fn shown_size_survives_content_updates_parking_and_dpi_changes() {
        let mut layout = layout("{}");
        layout.measure(Some(ContentSize {
            width: 440.0,
            height: 401.0,
        }));
        layout.lock();
        layout.measure(Some(ContentSize {
            width: 440.0,
            height: 2000.0,
        }));
        let display = Display {
            area: Rect {
                x: 0,
                y: 0,
                width: 3840,
                height: 2160,
            },
            scale: 2.0,
        };
        layout.set_display(display);
        assert_eq!(layout.placement().logical_size, (440, 401));
        assert_eq!(layout.placement().size, (880, 802));
        layout.set_display(Display {
            area: Rect {
                height: 200,
                ..display.area
            },
            scale: 2.0,
        });
        assert_eq!(layout.placement().logical_size.1, 68);
        layout.set_display(display);
        assert_eq!(layout.placement().logical_size.1, 401);
    }

    #[test]
    fn stale_width_and_invalid_measurements_do_not_corrupt_geometry() {
        let mut layout = layout("{}");
        assert!(!layout.measure(Some(ContentSize {
            width: 360.0,
            height: 600.0
        })));
        assert!(layout.measure(Some(ContentSize {
            width: 440.0,
            height: f64::NAN
        })));
        assert_eq!(layout.placement().logical_size, (440, 300));
        layout.measure(Some(ContentSize {
            width: 440.0,
            height: 400.0,
        }));
        layout.measure(None);
        assert_eq!(layout.placement().logical_size.1, 300);
    }

    #[test]
    fn explicit_height_keeps_percentage_and_pixel_semantics() {
        for (window, expected) in [(r#"{"height":100}"#, 100), (r#"{"height":"100%"}"#, 1040)] {
            let mut layout = layout(window);
            layout.measure(Some(ContentSize {
                width: 440.0,
                height: 600.0,
            }));
            assert_eq!(layout.placement().logical_size.1, expected);
        }
    }

    #[test]
    fn notification_limits_and_tiny_display_override_the_minimum() {
        let request =
            Request::parse(r#"{"version":"1.0","mode":"notify","content":{"title":"Test"}}"#)
                .unwrap();
        let mut layout = WindowLayout::new(
            &request,
            Display {
                area: Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1040,
                },
                scale: 1.0,
            },
        );
        for (height, expected) in [(10.0, 120), (2000.0, 480)] {
            layout.measure(Some(ContentSize {
                width: 360.0,
                height,
            }));
            assert_eq!(layout.placement().logical_size.1, expected);
        }
        layout.set_display(Display {
            area: Rect {
                x: 0,
                y: 0,
                width: 240,
                height: 100,
            },
            scale: 1.0,
        });
        assert_eq!(layout.placement().logical_size, (240, 68));
    }
}
