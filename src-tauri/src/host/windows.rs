// Native request-window adapter. Calls run on the coordinator's worker, never
// inside a UI callback or while holding the request registry lock.
use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use tauri::{
    window::Color, AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use uuid::Uuid;

use crate::placement::{Point, WindowPlacement};
use crate::protocol::{Mode, Request};
use crate::runtime;

#[derive(Debug, Clone, Serialize)]
pub(super) struct Observation {
    pub visible: bool,
    pub minimized: bool,
    pub cloaked: u32,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub webview_visible: bool,
    pub webview_size: (u32, u32),
}

impl Observation {
    pub fn system_hidden(&self) -> bool {
        self.cloaked & !1 != 0
    }
    pub fn healthy(&self) -> bool {
        self.visible
            && !self.minimized
            && self.cloaked == 0
            && self.webview_visible
            && self.size.0 > 0
            && self.size.1 > 0
            && self.webview_size.0.abs_diff(self.size.0) <= 2
            && self.webview_size.1.abs_diff(self.size.1) <= 2
    }
}

pub(super) fn create(
    app: &AppHandle,
    id: Uuid,
    request: &Request,
    placement: WindowPlacement,
) -> Result<(), String> {
    let notify = request.mode == Mode::Notify;
    let window =
        WebviewWindowBuilder::new(app, id.to_string(), WebviewUrl::App("index.html".into()))
            .title(&request.content.title)
            .inner_size(1.0, 1.0)
            .min_inner_size(1.0, 1.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .background_color(Color(0, 0, 0, 0))
            .always_on_top(request.window.always_on_top)
            .skip_taskbar(notify)
            .focused(!notify)
            .visible(false)
            .shadow(false)
            .build()
            .map_err(|e| e.to_string())?;
    resize(&window, placement)?;
    #[cfg(windows)]
    runtime::configure_frameless_windows_window(&window, request.window.is_maximized())?;
    // Hidden WebViews can throttle page startup. On Windows the HWND is shown
    // while DWM keeps its content cloaked until the renderer is ready.
    #[cfg(windows)]
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn resize(window: &WebviewWindow, placement: WindowPlacement) -> Result<(), String> {
    runtime::position_window(window, placement)?;
    runtime::size_window(window, placement)?;
    runtime::position_window(window, placement)?;
    // Round-trip through the event loop before measuring/applying the next step.
    let _ = window.inner_size().map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn position(window: &WebviewWindow, point: Point) -> Result<(), String> {
    window
        .set_position(PhysicalPosition::new(point.x, point.y))
        .map_err(|e| e.to_string())?;
    let actual = window.outer_position().map_err(|e| e.to_string())?;
    if actual.x != point.x || actual.y != point.y {
        return Err(format!(
            "position did not settle: expected {point:?}, got {actual:?}"
        ));
    }
    Ok(())
}

pub(super) fn reveal(window: &WebviewWindow) -> Result<Observation, String> {
    window.unminimize().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    runtime::reveal_windows_window(window)?;
    let observation = observe(window)?;
    if !observation.healthy() && !observation.system_hidden() {
        return Err(format!(
            "window did not become presentable: {observation:?}"
        ));
    }
    Ok(observation)
}

/// Hide is confirmed before the stack is compacted. Destroy can finish later:
/// the old HWND can no longer cover a newly promoted notification.
pub(super) fn close(app: &AppHandle, id: Uuid) -> Result<(), String> {
    let Some(window) = app.get_webview_window(&id.to_string()) else {
        return Ok(());
    };
    conceal(&window)?;
    window.destroy().map_err(|e| e.to_string())
}

pub(super) fn conceal(window: &WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())?;
    if window.is_visible().map_err(|e| e.to_string())? {
        return Err("closing window remained visible".into());
    }
    Ok(())
}

pub(super) fn restore(window: &WebviewWindow) -> Result<Observation, String> {
    #[cfg(windows)]
    {
        let size = window.inner_size().map_err(|e| e.to_string())?;
        let (sender, receiver) = mpsc::channel();
        window
            .with_webview(move |webview| {
                let result = (|| {
                    let controller = webview.controller();
                    let mut bounds = Default::default();
                    unsafe {
                        controller.Bounds(&mut bounds).map_err(|e| e.to_string())?;
                        bounds.left = 0;
                        bounds.top = 0;
                        bounds.right = size.width as i32;
                        bounds.bottom = size.height as i32;
                        controller.SetBounds(bounds).map_err(|e| e.to_string())?;
                        controller.SetIsVisible(true).map_err(|e| e.to_string())
                    }
                })();
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())??;
    }
    reveal(window)
}

pub(super) fn observe(window: &WebviewWindow) -> Result<Observation, String> {
    let visible = window.is_visible().map_err(|e| e.to_string())?;
    let minimized = window.is_minimized().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let point = window.outer_position().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        let mut cloaked = 0_u32;
        let result = unsafe {
            DwmGetWindowAttribute(
                window.hwnd().map_err(|e| e.to_string())?.0,
                DWMWA_CLOAKED as u32,
                std::ptr::from_mut(&mut cloaked).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if result < 0 {
            return Err(format!("DwmGetWindowAttribute failed: {result:#x}"));
        }
        let (sender, receiver) = mpsc::channel();
        window
            .with_webview(move |webview| {
                let result: Result<_, String> = (|| {
                    let controller = webview.controller();
                    let mut bounds = Default::default();
                    let mut visible = Default::default();
                    unsafe {
                        controller.Bounds(&mut bounds).map_err(|e| e.to_string())?;
                        controller
                            .IsVisible(&mut visible)
                            .map_err(|e| e.to_string())?;
                    }
                    Ok((
                        visible.as_bool(),
                        (
                            (bounds.right - bounds.left).max(0) as u32,
                            (bounds.bottom - bounds.top).max(0) as u32,
                        ),
                    ))
                })();
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        let (webview_visible, webview_size) = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())??;
        Ok(Observation {
            visible,
            minimized,
            cloaked,
            position: (point.x, point.y),
            size: (size.width, size.height),
            webview_visible,
            webview_size,
        })
    }
    #[cfg(not(windows))]
    Ok(Observation {
        visible,
        minimized,
        cloaked: 0,
        position: (point.x, point.y),
        size: (size.width, size.height),
        webview_visible: visible,
        webview_size: (size.width, size.height),
    })
}
