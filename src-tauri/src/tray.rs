use std::sync::LazyLock;

use tauri::image::Image;

pub const IDLE_FRAME: usize = 0;
const HOLD_TICKS: u8 = 40;
const ACTIVATION: &[usize] = &[1, 2, 3, 2, 1, 2, 3, 4, 5, 6];
const PULSE: &[usize] = &[7, 8, 8, 9, 6];
const RETURN: &[usize] = &[5, 4, 2, 0];

static FRAMES: LazyLock<Vec<Image<'static>>> = LazyLock::new(|| {
    [
        include_bytes!("../icons/tray/idle.png").as_slice(),
        include_bytes!("../icons/tray/takeoff.png").as_slice(),
        include_bytes!("../icons/tray/rise.png").as_slice(),
        include_bytes!("../icons/tray/peak.png").as_slice(),
        include_bytes!("../icons/tray/fall.png").as_slice(),
        include_bytes!("../icons/tray/land.png").as_slice(),
        include_bytes!("../icons/tray/active.png").as_slice(),
        include_bytes!("../icons/tray/pulse-crouch.png").as_slice(),
        include_bytes!("../icons/tray/pulse-peak.png").as_slice(),
        include_bytes!("../icons/tray/pulse-land.png").as_slice(),
    ]
    .into_iter()
    .map(|bytes| Image::from_bytes(bytes).expect("embedded tray frame must be a valid PNG").to_owned())
    .collect()
});

pub fn frame(index: usize) -> Image<'static> { FRAMES[index].clone() }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase { Idle, Activating(usize), Holding(u8), Pulsing(usize), Returning(usize) }

pub struct Animator {
    phase: Phase,
    current_frame: usize,
}

impl Default for Animator {
    fn default() -> Self { Self { phase: Phase::Idle, current_frame: IDLE_FRAME } }
}

impl Animator {
    /// Advance one 100 ms Host tick and return a frame only when the image changes.
    pub fn advance(&mut self, active: bool) -> Option<usize> {
        let (phase, target) = if active {
            match self.phase {
                Phase::Idle | Phase::Returning(_) => (Phase::Activating(1), Some(ACTIVATION[0])),
                Phase::Activating(next) => sequence_step(ACTIVATION, next, Phase::Activating, Phase::Holding(0)),
                Phase::Holding(ticks) if ticks + 1 >= HOLD_TICKS => (Phase::Pulsing(1), Some(PULSE[0])),
                Phase::Holding(ticks) => (Phase::Holding(ticks + 1), None),
                Phase::Pulsing(next) => sequence_step(PULSE, next, Phase::Pulsing, Phase::Holding(0)),
            }
        } else {
            match self.phase {
                Phase::Idle => (Phase::Idle, None),
                Phase::Returning(next) => sequence_step(RETURN, next, Phase::Returning, Phase::Idle),
                _ => (Phase::Returning(1), Some(RETURN[0])),
            }
        };
        self.phase = phase;
        let target = target.filter(|frame| *frame != self.current_frame)?;
        self.current_frame = target;
        Some(target)
    }
}

fn sequence_step(sequence: &[usize], next: usize, continuing: fn(usize) -> Phase, completed: Phase) -> (Phase, Option<usize>) {
    let following = next + 1;
    let phase = if following == sequence.len() { completed } else { continuing(following) };
    (phase, Some(sequence[next]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed_frames(animator: &mut Animator, active: bool, ticks: usize) -> Vec<usize> {
        (0..ticks).filter_map(|_| animator.advance(active)).collect()
    }

    #[test]
    fn activation_holds_then_pulses_without_redundant_frames() {
        let mut animator = Animator::default();
        let activation = changed_frames(&mut animator, true, ACTIVATION.len());
        assert_eq!(activation.first(), Some(&ACTIVATION[0]));
        assert_eq!(activation.last(), Some(&6));
        assert!(changed_frames(&mut animator, true, (HOLD_TICKS - 1) as usize).is_empty());
        assert_eq!(animator.advance(true), Some(PULSE[0]));
        let pulse = changed_frames(&mut animator, true, PULSE.len() - 1);
        assert_eq!(pulse.last(), Some(&6));
        assert!(changed_frames(&mut animator, true, (HOLD_TICKS - 1) as usize).is_empty());
        assert_eq!(animator.advance(true), Some(PULSE[0]));
    }

    #[test]
    fn inactive_reverses_to_idle_and_reactivation_restarts_jump() {
        let mut animator = Animator::default();
        changed_frames(&mut animator, true, ACTIVATION.len());
        assert_eq!(animator.advance(false), Some(RETURN[0]));
        let returned = changed_frames(&mut animator, false, RETURN.len() - 1);
        assert_eq!(returned.last(), Some(&IDLE_FRAME));
        assert_eq!(animator.advance(false), None);
        assert_eq!(animator.advance(true), Some(ACTIVATION[0]));
        assert_eq!(animator.advance(false), Some(RETURN[0]));
        assert_eq!(animator.advance(true), Some(ACTIVATION[0]));
    }

    #[test]
    fn embedded_frames_are_32px_with_transparent_corners() {
        assert_eq!(FRAMES.len(), 10);
        for (index, image) in FRAMES.iter().enumerate() {
            assert_eq!((image.width(), image.height()), (32, 32), "frame {index}");
            let rgba = image.rgba();
            assert_eq!(rgba[3], 0, "frame {index} top-left corner");
            assert_eq!(rgba[(32 * 32 - 1) * 4 + 3], 0, "frame {index} bottom-right corner");
        }
    }
}
