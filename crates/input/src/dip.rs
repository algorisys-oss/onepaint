//! Pen dip recovery (happypaint `input/pointer-router.ts`, v0.97.6 / v0.204.9).
//!
//! Tracing slowly and lightly, a pen flutters around its tip threshold: it
//! "lifts" for a moment while still moving on the surface, and the stroke
//! splits into dashes. The driver keeps reporting the pen's positions as
//! hover meanwhile. So a lift during a stroke is held back for a short grace
//! window; nearby hovers are buffered, the anchor sliding to follow them; a
//! re-contact near the last hover replays that path into the same stroke.
//! A real lift (the hovers stop, or the window's hard cap passes) commits the
//! end and drops the buffer, so no phantom ink is left. A re-contact with
//! nothing buffered is a new stroke: the end is never bridged blind.
//!
//! Sliding alone would follow a lifted pen anywhere, since hovers arrive
//! every few points. So a dip is also bounded in distance from the lift and in
//! speed (v0.204.19): a hop to the next stroke commits the lift instead of
//! being replayed as ink.

/// Commit this long after the hover stream stops (a real lift).
pub const RECONNECT_MS: f64 = 110.0;
/// Commit this long after the lift, whatever the hovers do (a resting pen).
pub const RECONNECT_MAX_MS: f64 = 500.0;
/// How near (points) a hover or re-contact must be to the sliding anchor.
pub const RECONNECT_DIST: f32 = 24.0;
/// A single dip's buffered path is capped.
pub const MAX_DIP_POINTS: usize = 256;
/// A dip stays this near (points) the lift point, not the sliding anchor.
pub const DIP_MAX_TRAVEL: f32 = 40.0;
/// A dip moves slower than this (points per ms, 350 pt/s) over the last
/// [`DIP_SPEED_WINDOW_MS`].
pub const DIP_MAX_SPEED: f32 = 0.35;
pub const DIP_SPEED_WINDOW_MS: f64 = 40.0;

#[derive(Clone, Debug)]
struct Pending<T> {
    /// Where the pen lifted: the stroke's end if it commits.
    lift: [f32; 2],
    anchor: [f32; 2],
    started: f64,
    last: f64,
    path: Vec<([f32; 2], T)>,
    /// When each `path` point arrived.
    stamps: Vec<f64>,
}

/// What a hover during the grace window does.
#[derive(Clone, Debug, PartialEq)]
pub enum Hover {
    /// Buffered (or out of range and left for the timer): swallow it.
    Held,
    /// Past the hard cap: commit the end at `lift`, then treat the hover as
    /// an ordinary one.
    Commit { lift: [f32; 2] },
}

/// What a re-contact during the grace window does.
#[derive(Clone, Debug, PartialEq)]
pub enum Contact<T> {
    /// The same stroke: extend it through the dip's path, then to here.
    Resume(Vec<([f32; 2], T)>),
    /// A new stroke: commit the end at `lift` first.
    Commit { lift: [f32; 2] },
}

/// Holds a pen stroke's end across tip dips.
#[derive(Clone, Debug)]
pub struct DipBridge<T> {
    pending: Option<Pending<T>>,
}

impl<T> Default for DipBridge<T> {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl<T> Pending<T> {
    /// A hover too far from the lift, or moving too fast, is the pen leaving
    /// for a new stroke, not a tip dip (`leftTheDip`).
    fn left_the_dip(&self, at: [f32; 2], ms: f64) -> bool {
        if (at[0] - self.lift[0]).hypot(at[1] - self.lift[1]) > DIP_MAX_TRAVEL {
            return true;
        }
        // Speed against the newest point at least a window old (else the
        // lift), over at least the window: one step is too noisy.
        let (from, t) = self
            .path
            .iter()
            .zip(&self.stamps)
            .rev()
            .find(|(_, t)| ms - **t >= DIP_SPEED_WINDOW_MS)
            .map_or((self.lift, self.started), |((p, _), t)| (*p, *t));
        let d = (at[0] - from[0]).hypot(at[1] - from[1]);
        d / (ms - t).max(DIP_SPEED_WINDOW_MS) as f32 > DIP_MAX_SPEED
    }
}

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= RECONNECT_DIST
}

impl<T: Clone> DipBridge<T> {
    /// A stroke's end is being held back.
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The pen lifted at `at` mid-stroke: hold the end back.
    pub fn lift(&mut self, at: [f32; 2], ms: f64) {
        self.pending = Some(Pending { lift: at, anchor: at, started: ms, last: ms, path: Vec::new(), stamps: Vec::new() });
    }

    /// The pen hovered at `at` while the end is held back.
    pub fn hover(&mut self, at: [f32; 2], sample: T, ms: f64) -> Hover {
        let Some(p) = self.pending.as_mut() else { return Hover::Held };
        if ms - p.started >= RECONNECT_MAX_MS {
            let lift = p.lift;
            self.pending = None;
            return Hover::Commit { lift };
        }
        // Wandered off: left for the timer, which a real lift lets run out.
        if near(at, p.anchor) {
            if p.left_the_dip(at, ms) {
                let lift = p.lift;
                self.pending = None;
                return Hover::Commit { lift };
            }
            if p.path.len() < MAX_DIP_POINTS {
                p.path.push((at, sample));
                p.stamps.push(ms);
            }
            (p.anchor, p.last) = (at, ms);
        }
        Hover::Held
    }

    /// The tip touched down at `at` while the end is held back.
    pub fn contact(&mut self, at: [f32; 2]) -> Contact<T> {
        let Some(p) = self.pending.take() else { return Contact::Resume(Vec::new()) };
        if !p.path.is_empty() && near(at, p.anchor) { Contact::Resume(p.path) } else { Contact::Commit { lift: p.lift } }
    }

    /// Time passing: `Some(lift)` when the window ran out and the end commits.
    pub fn tick(&mut self, ms: f64) -> Option<[f32; 2]> {
        let p = self.pending.as_ref()?;
        if ms - p.last >= RECONNECT_MS || ms - p.started >= RECONNECT_MAX_MS {
            return self.pending.take().map(|p| p.lift);
        }
        None
    }

    /// Commit now (the pen left range, the window lost focus): `Some(lift)`
    /// when an end was held back.
    pub fn flush(&mut self) -> Option<[f32; 2]> {
        self.pending.take().map(|p| p.lift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dip_while_tracing_resumes_the_stroke_through_its_path() {
        let mut b = DipBridge::default();
        b.lift([100.0, 100.0], 0.0);
        assert_eq!(b.hover([104.0, 100.0], 1, 16.0), Hover::Held);
        assert_eq!(b.hover([108.0, 101.0], 2, 32.0), Hover::Held);
        assert_eq!(b.tick(48.0), None, "the hovers keep the window open");
        assert_eq!(b.contact([112.0, 101.0]), Contact::Resume(vec![([104.0, 100.0], 1), ([108.0, 101.0], 2)]));
        assert!(!b.pending());
    }

    #[test]
    fn the_anchor_slides_with_the_hovers() {
        // A slow trace (~0.1 pt/ms) drifts past 24 points from the lift.
        let mut b = DipBridge::default();
        b.lift([10.0, 0.0], 1000.0);
        for (i, (x, t)) in [(20.0, 1100.0), (30.0, 1200.0), (40.0, 1300.0)].into_iter().enumerate() {
            assert_eq!(b.hover([x, 0.0], i, t), Hover::Held);
        }
        assert!(matches!(b.contact([40.0, 0.0]), Contact::Resume(p) if p.len() == 3));
    }

    // happypaint v0.204.19: a lift, a quick hop and a tap inked the hover
    // path, joining each new stroke to the last (field video 2026-11-06).
    #[test]
    fn a_fast_hop_after_a_lift_is_a_new_stroke() {
        let mut b = DipBridge::default();
        b.lift([10.0, 0.0], 1000.0);
        // Speed is taken over at least 40 ms: 10 points is 0.25 pt/ms, 20 is 0.5.
        assert_eq!(b.hover([20.0, 0.0], 0u8, 1008.0), Hover::Held);
        assert_eq!(b.hover([30.0, 0.0], 0, 1016.0), Hover::Commit { lift: [10.0, 0.0] });
        assert!(!b.pending());
    }

    #[test]
    fn a_slow_hover_that_wanders_too_far_is_a_new_stroke() {
        let mut b = DipBridge::default();
        b.lift([10.0, 0.0], 1000.0);
        let mut out = Vec::new();
        for i in 1..=7 {
            out.push(b.hover([10.0 + i as f32 * 10.0, 0.0], i, 1000.0 + i as f64 * 60.0));
        }
        // 0.17 pt/ms is slow enough; the fifth hover is 50 points from the lift.
        assert_eq!(out[..4], [Hover::Held, Hover::Held, Hover::Held, Hover::Held]);
        assert_eq!(out[4], Hover::Commit { lift: [10.0, 0.0] });
        assert!(!b.pending());
    }

    #[test]
    fn speed_is_measured_over_the_window_not_one_step() {
        // Jitter: one 6-point step in 4 ms (1.5 pt/ms) inside a slow trace.
        let mut b = DipBridge::default();
        b.lift([0.0, 0.0], 0.0);
        assert_eq!(b.hover([2.0, 0.0], 0u8, 40.0), Hover::Held);
        assert_eq!(b.hover([4.0, 0.0], 0, 80.0), Hover::Held);
        assert_eq!(b.hover([10.0, 0.0], 0, 84.0), Hover::Held, "8 points over 44 ms");
    }

    #[test]
    fn a_real_lift_commits_once_the_hovers_stop() {
        let mut b: DipBridge<u8> = DipBridge::default();
        b.lift([50.0, 50.0], 0.0);
        b.hover([52.0, 50.0], 0, 10.0);
        assert_eq!(b.tick(100.0), None);
        assert_eq!(b.tick(120.0), Some([50.0, 50.0]), "ends where it lifted, the hover dropped");
        assert!(!b.pending());
    }

    #[test]
    fn a_resting_pen_commits_at_the_hard_cap() {
        let mut b = DipBridge::default();
        b.lift([0.0, 0.0], 0.0);
        let mut t = 0.0;
        while t < 480.0 {
            t += 16.0;
            assert_eq!(b.hover([1.0, 0.0], 0, t), Hover::Held);
            assert_eq!(b.tick(t), None);
        }
        assert_eq!(b.hover([1.0, 0.0], 0, 500.0), Hover::Commit { lift: [0.0, 0.0] });
    }

    #[test]
    fn a_re_contact_with_no_dip_path_is_a_new_stroke() {
        // Never bridge blind: lift, then tap right next to it.
        let mut b: DipBridge<u8> = DipBridge::default();
        b.lift([10.0, 10.0], 0.0);
        assert_eq!(b.contact([12.0, 10.0]), Contact::Commit { lift: [10.0, 10.0] });
    }

    #[test]
    fn a_far_re_contact_is_a_new_stroke() {
        let mut b = DipBridge::default();
        b.lift([10.0, 10.0], 0.0);
        b.hover([14.0, 10.0], 0u8, 16.0);
        // A hover that wandered off is not buffered and does not move the anchor.
        b.hover([200.0, 10.0], 0, 32.0);
        assert_eq!(b.contact([200.0, 10.0]), Contact::Commit { lift: [10.0, 10.0] });
    }

    #[test]
    fn flush_commits_a_held_end() {
        let mut b: DipBridge<u8> = DipBridge::default();
        assert_eq!(b.flush(), None);
        b.lift([3.0, 4.0], 0.0);
        assert_eq!(b.flush(), Some([3.0, 4.0]));
        assert!(!b.pending());
    }

    #[test]
    fn a_dip_path_is_capped() {
        let mut b = DipBridge::default();
        b.lift([0.0, 0.0], 0.0);
        for i in 0..400 {
            b.hover([0.5, 0.0], i, 1.0);
        }
        assert!(matches!(b.contact([0.0, 0.0]), Contact::Resume(p) if p.len() == MAX_DIP_POINTS));
    }
}
