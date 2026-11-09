//! Thin platform window helpers used by the persistent Host.

use tauri::{LogicalSize, PhysicalPosition, WebviewWindow};

use crate::placement::WindowPlacement;
use crate::protocol::{Screen, ScreenArea};
use crate::placement::Display;

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 12;

/// WebView2 can own keyboard focus in a child HWND while Tao's cached outer
/// window focus is false. Check the actual foreground top-level window.
#[cfg(windows)]
pub(crate) fn is_window_foreground(window: &WebviewWindow) -> Result<bool, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, IsWindowVisible};
    let hwnd = windows_hwnd(window)?;
    Ok(unsafe { GetForegroundWindow() == hwnd && IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0 })
}

#[cfg(not(windows))]
pub(crate) fn is_window_foreground(window: &WebviewWindow) -> Result<bool, String> {
    Ok(window.is_focused().map_err(|e| e.to_string())?
        && window.is_visible().map_err(|e| e.to_string())?
        && !window.is_minimized().map_err(|e| e.to_string())?)
}

#[cfg(windows)]
const NATIVE_FRAME_STYLES: u32 = {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };
    WS_CAPTION | WS_MAXIMIZEBOX | WS_MINIMIZEBOX | WS_SYSMENU | WS_THICKFRAME
};

#[cfg(windows)]
pub(crate) fn keep_request_window_frameless(window: &WebviewWindow) -> Result<(), String> {
    use std::{sync::mpsc, time::Duration};
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;

    // Subclass registration must run on the thread that owns the HWND.
    let (sender, receiver) = mpsc::channel();
    let request_window = window.clone();
    window
        .run_on_main_thread(move || {
            let result = windows_hwnd(&request_window).and_then(|hwnd| {
                if unsafe { SetWindowSubclass(hwnd, Some(frameless_window_proc), 0, 0) } == 0 {
                    return Err("failed to install frameless request window handler".into());
                }
                Ok(())
            });
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver.recv_timeout(Duration::from_secs(2)).map_err(|error| error.to_string())?
}

#[cfg(windows)]
unsafe extern "system" fn frameless_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    subclass_id: usize,
    _data: usize,
) -> isize {
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, STYLESTRUCT, WM_NCACTIVATE, WM_NCDESTROY, WM_STYLECHANGING,
    };

    // Right-click can activate a notification and paint a stale caption into
    // the transparent margin even with WS_CAPTION cleared. Win32 defines -1
    // as activation without repaint. Forward it so Tao still updates focus.
    if message == WM_NCACTIVATE {
        return unsafe { DefSubclassProc(hwnd, message, wparam, -1) };
    }

    // Tao can restore the native frame bits on show/hide after initial sizing.
    if message == WM_STYLECHANGING && wparam as i32 == GWL_STYLE && lparam != 0 {
        let styles = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
        styles.styleNew &= !NATIVE_FRAME_STYLES;
    }
    if message == WM_NCDESTROY {
        unsafe { RemoveWindowSubclass(hwnd, Some(frameless_window_proc), subclass_id) };
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(windows)]
pub(crate) fn configure_frameless_windows_window(window: &WebviewWindow, maximized: bool) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER};
    let hwnd = windows_hwnd(window)?;
    set_window_cloaked(hwnd, true)?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let radius = (14.0 * window.scale_factor().map_err(|e| e.to_string())?).round().max(1.0) as i32;
    let width = i32::try_from(size.width).map_err(|_| "window width is too large")?;
    let height = i32::try_from(size.height).map_err(|_| "window height is too large")?;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & !NATIVE_FRAME_STYLES;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
        if SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) == 0 { return Err("SetWindowPos failed".into()); }
        let region = if maximized { CreateRectRgn(0, 0, width + 1, height + 1) } else { CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2) };
        if region.is_null() { return Err("CreateRoundRectRgn failed".into()); }
        if SetWindowRgn(hwnd, region, 1) == 0 { let _ = DeleteObject(region); return Err("SetWindowRgn failed".into()); }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn reveal_windows_window(window: &WebviewWindow) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;
    let hwnd = windows_hwnd(window)?;
    unsafe { let _ = DwmFlush(); }
    set_window_cloaked(hwnd, false)?;
    Ok(())
}

#[cfg(windows)]
fn windows_hwnd(window: &WebviewWindow) -> Result<windows_sys::Win32::Foundation::HWND, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else { return Err("window does not expose a Win32 handle".into()) };
    Ok(handle.hwnd.get() as HWND)
}

#[cfg(windows)]
fn set_window_cloaked(hwnd: windows_sys::Win32::Foundation::HWND, cloaked: bool) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
    let value = i32::from(cloaked);
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK as u32,
            std::ptr::from_ref(&value).cast(),
            std::mem::size_of_val(&value) as u32,
        )
    };
    if result < 0 { Err(format!("DwmSetWindowAttribute(DWMWA_CLOAK={cloaked}) failed: {result:#x}")) } else { Ok(()) }
}

#[cfg(not(windows))]
pub(crate) fn reveal_windows_window(_window: &WebviewWindow) -> Result<(), String> { Ok(()) }

pub(crate) fn notification_display() -> Result<Display, String> {
    resolve_display(Screen::Primary, ScreenArea::WorkArea)
}

#[cfg(windows)]
pub(crate) fn resolve_display(screen: Screen, area_kind: ScreenArea) -> Result<Display, String> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut cursor = POINT { x: 0, y: 0 };
    unsafe {
        if screen == Screen::Cursor && GetCursorPos(&mut cursor) == 0 { return Err("GetCursorPos failed".into()); }
        let point = if screen == Screen::Primary { POINT { x: i32::MIN, y: i32::MIN } } else { cursor };
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY);
        let mut dpi_x = 0_u32;
        let mut dpi_y = 0_u32;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) < 0 || dpi_x == 0 {
            return Err("GetDpiForMonitor failed".into());
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: Default::default(), rcWork: Default::default(), dwFlags: 0 };
        if GetMonitorInfoW(monitor, &mut info) == 0 { return Err("GetMonitorInfoW failed".into()); }
        let rect = match area_kind { ScreenArea::WorkArea => info.rcWork, ScreenArea::Screen => info.rcMonitor };
        let area = crate::placement::Rect { x: rect.left, y: rect.top, width: (rect.right - rect.left).max(0) as u32, height: (rect.bottom - rect.top).max(0) as u32 };
        if area.width == 0 || area.height == 0 { return Err("target display area is empty".into()); }
        Ok(Display { area, scale: dpi_x as f64 / 96.0 })
    }
}

#[cfg(not(windows))]
pub(crate) fn resolve_display(_screen: Screen, _area: ScreenArea) -> Result<Display, String> {
    Ok(Display { area: crate::placement::Rect { x: 0, y: 0, width: 1920, height: 1080 }, scale: 1.0 })
}

pub(crate) fn position_window(window: &WebviewWindow, placement: WindowPlacement) -> Result<(), String> {
    window.set_position(PhysicalPosition::new(placement.position.x, placement.position.y)).map_err(|e| e.to_string())
}

/// Native coordinates are physical, but WebView layout dimensions are logical.
pub(crate) fn size_window(window: &WebviewWindow, placement: WindowPlacement) -> Result<(), String> {
    window.set_size(LogicalSize::new(placement.logical_size.0 as f64, placement.logical_size.1 as f64)).map_err(|e| e.to_string())
}
