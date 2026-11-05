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

/// Commit this long after the hover stream stops (a real lift).
pub const RECONNECT_MS: f64 = 110.0;
/// Commit this long after the lift, whatever the hovers do (a resting pen).
pub const RECONNECT_MAX_MS: f64 = 500.0;
/// How near (points) a hover or re-contact must be to the sliding anchor.
pub const RECONNECT_DIST: f32 = 24.0;
/// A single dip's buffered path is capped.
pub const MAX_DIP_POINTS: usize = 256;

#[derive(Clone, Debug)]
struct Pending<T> {
    /// Where the pen lifted: the stroke's end if it commits.
    lift: [f32; 2],
    anchor: [f32; 2],
    started: f64,
    last: f64,
    path: Vec<([f32; 2], T)>,
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
        self.pending = Some(Pending { lift: at, anchor: at, started: ms, last: ms, path: Vec::new() });
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
            if p.path.len() < MAX_DIP_POINTS {
                p.path.push((at, sample));
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
        // A long flutter wanders well past 24 points from the lift.
        let mut b = DipBridge::default();
        b.lift([0.0, 0.0], 0.0);
        for i in 1..=10 {
            b.hover([i as f32 * 10.0, 0.0], i, i as f64 * 16.0);
        }
        assert!(matches!(b.contact([105.0, 0.0]), Contact::Resume(p) if p.len() == 10));
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
