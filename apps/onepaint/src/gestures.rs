//! Procreate's touch vocabulary (happypaint `input/`): multi-finger taps,
//! a three-finger swipe, touch-and-hold eyedropper, two-finger hold to
//! repeat undo, and palm rejection while a pen is in use. Pure: fed touch
//! samples and `tick`s, it reports what happened.

use std::collections::HashMap;

/// A tap must end within this and move less than `TAP_SLOP` px per finger.
pub const TAP_MS: f64 = 300.0;
pub const TAP_SLOP: f32 = 12.0;
/// A three-finger swipe needs this much average downward travel.
pub const SWIPE_PX: f32 = 80.0;
/// Fingers are ignored for this long after the pen was last used.
pub const PALM_MS: f64 = 1500.0;
/// A still finger becomes the eyedropper after this (`gesture-recognizer.ts`).
pub const HOLD_MS: f64 = 400.0;
pub const HOLD_SLOP: f32 = 8.0;
/// Two still fingers start repeating undo after this, then every `UNDO_REPEAT_MS`.
pub const UNDO_REPEAT_DELAY_MS: f64 = 600.0;
pub const UNDO_REPEAT_MS: f64 = 150.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// More than one finger is down: abandon any stroke the first one began.
    MultiTouchStarted,
    Tap(usize),
    SwipeDown3,
    /// Touch-and-hold eyedropper: cancel the tiny stroke and pick here.
    HoldBegin([f32; 2]),
    HoldMove([f32; 2]),
    HoldEnd,
}

#[derive(Clone, Copy)]
struct Touch {
    start: [f32; 2],
    pos: [f32; 2],
    start_ms: f64,
    pen: bool,
}

#[derive(Default)]
pub struct Gestures {
    touches: HashMap<u64, Touch>,
    /// Most fingers down at once in the current sequence.
    peak: usize,
    first_ms: f64,
    moved: bool,
    /// Finished fingers' travel (dy) in this sequence.
    travel: Vec<f32>,
    last_pen_ms: Option<f64>,
    /// The current sequence is a resting palm: ignored.
    palm: bool,
    /// The eyedropper hold is live.
    holding: bool,
    /// The single finger strayed beyond `HOLD_SLOP`: no hold this sequence.
    hold_off: bool,
    /// When two fingers landed, and the next repeated undo (once started).
    multi_ms: f64,
    next_undo_ms: Option<f64>,
}

impl Gestures {
    /// True while fingers are down in a multi-finger or rejected sequence:
    /// the canvas should not paint with the emulated pointer.
    pub fn suppress_pointer(&self) -> bool {
        self.palm || self.peak >= 2 || self.holding
    }

    pub fn start(&mut self, id: u64, pos: [f32; 2], ms: f64, pen: bool) -> Vec<Gesture> {
        if pen {
            self.last_pen_ms = Some(ms);
        }
        if self.touches.is_empty() {
            (self.peak, self.first_ms, self.moved, self.palm) = (0, ms, false, false);
            (self.holding, self.hold_off, self.next_undo_ms) = (false, pen, None);
            self.travel.clear();
            // Palm rejection: a finger landing while the pen was just in use.
            self.palm = !pen && self.last_pen_ms.is_some_and(|t| ms - t < PALM_MS);
        }
        self.touches.insert(id, Touch { start: pos, pos, start_ms: ms, pen });
        let before = self.peak;
        self.peak = self.peak.max(self.touches.len());
        let mut out = Vec::new();
        if self.touches.len() >= 2 {
            self.hold_off = true;
            if std::mem::take(&mut self.holding) {
                out.push(Gesture::HoldEnd);
            }
        }
        if self.touches.len() == 2 {
            self.multi_ms = ms;
        }
        if self.peak >= 2 && before < 2 && !self.palm {
            out.push(Gesture::MultiTouchStarted);
        }
        out
    }

    /// Time passing with fingers down: the hold eyedropper and repeated undo.
    pub fn tick(&mut self, ms: f64) -> Vec<Gesture> {
        if self.palm {
            return Vec::new();
        }
        match (self.touches.len(), self.peak) {
            (1, 1) if !self.holding && !self.hold_off && ms - self.first_ms >= HOLD_MS => {
                self.holding = true;
                let t = self.touches.values().next().map_or([0.0; 2], |t| t.pos);
                vec![Gesture::HoldBegin(t)]
            }
            (2, 2) if !self.moved => {
                let due = self.next_undo_ms.unwrap_or(self.multi_ms + UNDO_REPEAT_DELAY_MS);
                if ms >= due {
                    self.next_undo_ms = Some(ms + UNDO_REPEAT_MS);
                    vec![Gesture::Tap(2)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    /// Fingers are down: the app should keep ticking.
    pub fn active(&self) -> bool {
        !self.touches.is_empty()
    }

    pub fn moved(&mut self, id: u64, pos: [f32; 2], ms: f64) -> Vec<Gesture> {
        let Some(t) = self.touches.get_mut(&id) else { return Vec::new() };
        t.pos = pos;
        if t.pen {
            self.last_pen_ms = Some(ms);
        }
        let d = (pos[0] - t.start[0]).hypot(pos[1] - t.start[1]);
        if d > TAP_SLOP {
            self.moved = true;
        }
        if d > HOLD_SLOP {
            self.hold_off = true;
        }
        if self.holding { vec![Gesture::HoldMove(pos)] } else { Vec::new() }
    }

    pub fn end(&mut self, id: u64, ms: f64) -> Vec<Gesture> {
        let Some(t) = self.touches.remove(&id) else { return Vec::new() };
        if t.pen {
            self.last_pen_ms = Some(ms);
        }
        self.travel.push(t.pos[1] - t.start[1]);
        let _ = t.start_ms;
        if std::mem::take(&mut self.holding) {
            return vec![Gesture::HoldEnd];
        }
        if !self.touches.is_empty() || self.palm {
            return Vec::new();
        }
        if self.next_undo_ms.is_some() {
            // The held undo already fired; the release is not another tap.
            return Vec::new();
        }
        let n = self.peak;
        if n == 3 && self.travel.len() >= 3 {
            let avg = self.travel.iter().sum::<f32>() / self.travel.len() as f32;
            if avg >= SWIPE_PX {
                return vec![Gesture::SwipeDown3];
            }
        }
        if n >= 2 && !self.moved && ms - self.first_ms <= TAP_MS {
            return vec![Gesture::Tap(n)];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_and_three_finger_taps() {
        let mut g = Gestures::default();
        assert!(g.start(1, [10.0, 10.0], 0.0, false).is_empty());
        assert_eq!(g.start(2, [60.0, 10.0], 20.0, false), vec![Gesture::MultiTouchStarted]);
        assert!(g.suppress_pointer());
        assert!(g.end(1, 120.0).is_empty());
        assert_eq!(g.end(2, 130.0), vec![Gesture::Tap(2)]);
        for i in 0..3 {
            g.start(i, [i as f32 * 40.0, 0.0], 1000.0, false);
        }
        g.end(0, 1100.0);
        g.end(1, 1100.0);
        assert_eq!(g.end(2, 1150.0), vec![Gesture::Tap(3)]);
    }

    #[test]
    fn slow_or_moving_touches_are_not_taps() {
        let mut g = Gestures::default();
        g.start(1, [0.0, 0.0], 0.0, false);
        g.start(2, [50.0, 0.0], 0.0, false);
        g.moved(2, [90.0, 0.0], 50.0);
        g.end(1, 100.0);
        assert!(g.end(2, 100.0).is_empty(), "a pinch is not a tap");
        g.start(1, [0.0, 0.0], 1000.0, false);
        g.start(2, [50.0, 0.0], 1000.0, false);
        g.end(1, 1500.0);
        assert!(g.end(2, 1500.0).is_empty(), "too slow");
    }

    #[test]
    fn three_finger_swipe_down() {
        let mut g = Gestures::default();
        for i in 0..3 {
            g.start(i, [i as f32 * 40.0, 100.0], 0.0, false);
        }
        for i in 0..3 {
            g.moved(i, [i as f32 * 40.0, 220.0], 150.0);
        }
        g.end(0, 200.0);
        g.end(1, 200.0);
        assert_eq!(g.end(2, 200.0), vec![Gesture::SwipeDown3]);
    }

    #[test]
    fn touch_and_hold_is_the_eyedropper() {
        let mut g = Gestures::default();
        g.start(1, [100.0, 100.0], 0.0, false);
        assert!(g.tick(300.0).is_empty());
        assert_eq!(g.tick(410.0), vec![Gesture::HoldBegin([100.0, 100.0])]);
        assert!(g.suppress_pointer());
        assert!(g.tick(500.0).is_empty(), "begins once");
        assert_eq!(g.moved(1, [140.0, 100.0], 600.0), vec![Gesture::HoldMove([140.0, 100.0])]);
        assert_eq!(g.end(1, 700.0), vec![Gesture::HoldEnd]);
        // A finger that strays first is a stroke, never a hold.
        g.start(1, [0.0, 0.0], 1000.0, false);
        g.moved(1, [20.0, 0.0], 1100.0);
        assert!(g.tick(1500.0).is_empty());
        g.end(1, 1600.0);
        // The pen never holds.
        g.start(9, [0.0, 0.0], 5000.0, true);
        assert!(g.tick(6000.0).is_empty());
    }

    #[test]
    fn two_still_fingers_repeat_undo() {
        let mut g = Gestures::default();
        g.start(1, [0.0, 0.0], 0.0, false);
        g.start(2, [60.0, 0.0], 10.0, false);
        assert!(g.tick(500.0).is_empty());
        assert_eq!(g.tick(620.0), vec![Gesture::Tap(2)]);
        assert!(g.tick(700.0).is_empty());
        assert_eq!(g.tick(780.0), vec![Gesture::Tap(2)]);
        g.end(1, 800.0);
        assert!(g.end(2, 800.0).is_empty(), "no extra undo on release");
    }

    #[test]
    fn palms_are_ignored_right_after_the_pen() {
        let mut g = Gestures::default();
        g.start(9, [5.0, 5.0], 0.0, true);
        g.end(9, 500.0);
        g.start(1, [200.0, 200.0], 800.0, false);
        assert!(g.suppress_pointer(), "a finger 300 ms after the pen is a palm");
        g.start(2, [260.0, 200.0], 810.0, false);
        g.end(1, 900.0);
        assert!(g.end(2, 900.0).is_empty(), "no tap from a palm");
        g.start(1, [0.0, 0.0], 5000.0, false);
        assert!(!g.suppress_pointer(), "fingers work again later");
    }
}
