//! Thin platform window helpers used by the persistent Host.

use tauri::{LogicalSize, PhysicalPosition, WebviewWindow};

use crate::placement::WindowPlacement;
use crate::protocol::{Request, Screen, ScreenArea};

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 12;

#[cfg(windows)]
pub(crate) fn configure_frameless_windows_window(window: &WebviewWindow, maximized: bool) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME};
    let hwnd = windows_hwnd(window)?;
    set_window_cloaked(hwnd, true)?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let radius = (14.0 * window.scale_factor().map_err(|e| e.to_string())?).round().max(1.0) as i32;
    let width = i32::try_from(size.width).map_err(|_| "window width is too large")?;
    let height = i32::try_from(size.height).map_err(|_| "window height is too large")?;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
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

#[cfg(windows)]
pub(crate) fn resolve_window_placement(request: &Request) -> Result<WindowPlacement, String> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut cursor = POINT { x: 0, y: 0 };
    unsafe {
        if request.window.screen == Screen::Cursor && GetCursorPos(&mut cursor) == 0 { return Err("GetCursorPos failed".into()); }
        let point = if request.window.screen == Screen::Primary { POINT { x: i32::MIN, y: i32::MIN } } else { cursor };
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY);
        let mut dpi_x = 0_u32;
        let mut dpi_y = 0_u32;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) < 0 || dpi_x == 0 {
            return Err("GetDpiForMonitor failed".into());
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: Default::default(), rcWork: Default::default(), dwFlags: 0 };
        if GetMonitorInfoW(monitor, &mut info) == 0 { return Err("GetMonitorInfoW failed".into()); }
        let rect = match request.window.area { ScreenArea::WorkArea => info.rcWork, ScreenArea::Screen => info.rcMonitor };
        let area = crate::placement::Rect { x: rect.left, y: rect.top, width: (rect.right - rect.left).max(0) as u32, height: (rect.bottom - rect.top).max(0) as u32 };
        if area.width == 0 || area.height == 0 { return Err("target display area is empty".into()); }
        Ok(crate::placement::resolve(request, area, dpi_x as f64 / 96.0, 16))
    }
}

#[cfg(not(windows))]
pub(crate) fn resolve_window_placement(request: &Request) -> Result<WindowPlacement, String> {
    Ok(crate::placement::resolve(request, crate::placement::Rect { x: 0, y: 0, width: 1920, height: 1080 }, 1.0, 16))
}

pub(crate) fn position_window(window: &WebviewWindow, placement: WindowPlacement) -> Result<(), String> {
    window.set_position(PhysicalPosition::new(placement.position.x, placement.position.y)).map_err(|e| e.to_string())
}

/// Native coordinates are physical, but WebView layout dimensions are logical.
pub(crate) fn size_window(window: &WebviewWindow, placement: WindowPlacement) -> Result<(), String> {
    window.set_size(LogicalSize::new(placement.logical_size.0 as f64, placement.logical_size.1 as f64)).map_err(|e| e.to_string())
}
