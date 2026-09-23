use std::sync::LazyLock;
use std::time::Duration;

use tauri::{image::Image, AppHandle};

pub const IDLE_FRAME: usize = 0;
pub const FRAME_INTERVAL: Duration = Duration::from_millis(120);
const ACTIVATION: &[usize] = &[1, 2, 3, 4, 5, 6];
const PLAY_ROUNDS: usize = 3;
const REST_TICKS: u8 = 10;

static FRAMES: LazyLock<Vec<Image<'static>>> = LazyLock::new(|| decode_frames([
    include_bytes!("../icons/tray/0-idle.png").as_slice(),
    include_bytes!("../icons/tray/1-takeoff.png").as_slice(),
    include_bytes!("../icons/tray/2-rise.png").as_slice(),
    include_bytes!("../icons/tray/3-peak.png").as_slice(),
    include_bytes!("../icons/tray/4-fall.png").as_slice(),
    include_bytes!("../icons/tray/5-land.png").as_slice(),
    include_bytes!("../icons/tray/6-active.png").as_slice(),
]));

fn decode_frames(bytes: [&'static [u8]; 7]) -> Vec<Image<'static>> {
    bytes
        .into_iter()
        .map(|bytes| Image::from_bytes(bytes).expect("embedded tray frame must be a valid PNG").to_owned())
        .collect()
}

pub fn frame(index: usize) -> Image<'static> { FRAMES[index].clone() }

pub struct FrameRenderer {
    app: AppHandle,
    #[cfg(windows)]
    native: Option<WindowsFrameRenderer>,
}

impl FrameRenderer {
    pub fn new(app: AppHandle) -> Self {
        #[cfg(windows)]
        let native = match WindowsFrameRenderer::attach() {
            Ok(renderer) => Some(renderer),
            Err(error) => { eprintln!("Popu native tray renderer unavailable ({error}); using Tauri fallback"); None }
        };
        Self {
            app,
            #[cfg(windows)]
            native,
        }
    }

    pub fn render(&self, index: usize) {
        #[cfg(windows)]
        if self.native.as_ref().is_some_and(|renderer| renderer.render(index)) { return; }
        if let Some(icon) = self.app.tray_by_id("popu") { let _ = icon.set_icon(Some(frame(index))); }
    }
}

#[cfg(windows)]
struct WindowsFrameRenderer {
    hwnd: isize,
    tray_id: u32,
    icons: Vec<isize>,
}

#[cfg(windows)]
impl WindowsFrameRenderer {
    fn attach() -> Result<Self, String> {
        use windows_sys::Win32::UI::WindowsAndMessaging::CreateIcon;
        let (hwnd, tray_id) = find_tauri_tray().ok_or("Tauri tray window not found")?;
        let mut icons = Vec::with_capacity(FRAMES.len());
        for image in FRAMES.iter() {
            let mut bgra = image.rgba().to_vec();
            let mut and_mask = Vec::with_capacity(bgra.len() / 4);
            for pixel in bgra.chunks_exact_mut(4) {
                and_mask.push(pixel[3].wrapping_sub(u8::MAX));
                pixel.swap(0, 2);
            }
            let icon = unsafe { CreateIcon(std::ptr::null_mut(), image.width() as i32, image.height() as i32, 1, 32, and_mask.as_ptr(), bgra.as_ptr()) };
            if icon.is_null() {
                for icon in icons { unsafe { let _ = windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(icon as _); } }
                return Err(std::io::Error::last_os_error().to_string());
            }
            icons.push(icon as isize);
        }
        Ok(Self { hwnd, tray_id, icons })
    }

    fn render(&self, index: usize) -> bool {
        use std::mem::size_of;
        use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIM_MODIFY, NOTIFYICONDATAW};
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd as _,
            uID: self.tray_id,
            uFlags: NIF_ICON,
            hIcon: self.icons[index] as _,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) != 0 }
    }
}

#[cfg(windows)]
impl Drop for WindowsFrameRenderer {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;
        for icon in self.icons.drain(..) { unsafe { let _ = DestroyIcon(icon as _); } }
    }
}

#[cfg(windows)]
fn find_tauri_tray() -> Option<(isize, u32)> {
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};

    let class: Vec<u16> = "tray_icon_app\0".encode_utf16().collect();
    let mut hwnd = std::ptr::null_mut();
    loop {
        hwnd = unsafe { FindWindowExW(std::ptr::null_mut(), hwnd, class.as_ptr(), std::ptr::null()) };
        if hwnd.is_null() { return None; }
        let mut process_id = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut process_id); }
        if process_id != unsafe { GetCurrentProcessId() } { continue; }
        for tray_id in 1..=32 {
            let identifier = NOTIFYICONIDENTIFIER {
                cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: hwnd,
                uID: tray_id,
                ..unsafe { std::mem::zeroed() }
            };
            let mut rect: RECT = unsafe { std::mem::zeroed() };
            if unsafe { Shell_NotifyIconGetRect(&identifier, &mut rect) } >= 0 { return Some((hwnd as isize, tray_id)); }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase { Idle, Playing { next: usize, completed_rounds: usize }, Resting(u8) }

pub struct Animator {
    phase: Phase,
    current_frame: usize,
}

impl Default for Animator {
    fn default() -> Self { Self { phase: Phase::Idle, current_frame: IDLE_FRAME } }
}

impl Animator {
    /// Advance one animation tick and return a frame only when the image changes.
    pub fn advance(&mut self, active: bool) -> Option<usize> {
        let (phase, target) = if !active {
            (Phase::Idle, Some(IDLE_FRAME))
        } else {
            match self.phase {
                Phase::Idle => (Phase::Playing { next: 1, completed_rounds: 0 }, Some(ACTIVATION[0])),
                Phase::Playing { next, completed_rounds } => {
                    let following = next + 1;
                    let phase = if following < ACTIVATION.len() {
                        Phase::Playing { next: following, completed_rounds }
                    } else if completed_rounds + 1 < PLAY_ROUNDS {
                        Phase::Playing { next: 0, completed_rounds: completed_rounds + 1 }
                    } else {
                        Phase::Resting(0)
                    };
                    (phase, Some(ACTIVATION[next]))
                }
                Phase::Resting(ticks) if ticks + 1 >= REST_TICKS => {
                    (Phase::Playing { next: 1, completed_rounds: 0 }, Some(ACTIVATION[0]))
                }
                Phase::Resting(ticks) => (Phase::Resting(ticks + 1), None),
            }
        };
        self.phase = phase;
        let target = target.filter(|frame| *frame != self.current_frame)?;
        self.current_frame = target;
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed_frames(animator: &mut Animator, active: bool, ticks: usize) -> Vec<usize> {
        (0..ticks).filter_map(|_| animator.advance(active)).collect()
    }

    #[test]
    fn activation_plays_three_rounds_then_rests() {
        let mut animator = Animator::default();
        let played = changed_frames(&mut animator, true, ACTIVATION.len() * PLAY_ROUNDS);
        let expected: Vec<_> = (0..PLAY_ROUNDS).flat_map(|_| ACTIVATION.iter().copied()).collect();
        assert_eq!(played, expected);
        assert!(changed_frames(&mut animator, true, (REST_TICKS - 1) as usize).is_empty());
        assert_eq!(animator.advance(true), Some(ACTIVATION[0]));
    }

    #[test]
    fn inactive_resets_to_idle_and_reactivation_replays() {
        let mut animator = Animator::default();
        changed_frames(&mut animator, true, ACTIVATION.len() * PLAY_ROUNDS);
        assert_eq!(animator.advance(false), Some(IDLE_FRAME));
        assert_eq!(animator.advance(false), None);
        assert_eq!(animator.advance(true), Some(ACTIVATION[0]));
        assert_eq!(animator.advance(false), Some(IDLE_FRAME));
        assert_eq!(animator.advance(true), Some(ACTIVATION[0]));
    }

    #[test]
    fn embedded_frames_are_32px_with_transparent_corners() {
        assert_eq!(FRAMES.len(), 7);
        for (index, image) in FRAMES.iter().enumerate() {
            assert_eq!((image.width(), image.height()), (32, 32), "frame {index}");
            let rgba = image.rgba();
            assert_eq!(rgba[3], 0, "frame {index} top-left corner");
            assert_eq!(rgba[(32 * 32 - 1) * 4 + 3], 0, "frame {index} bottom-right corner");
        }
    }
}
