//! Draw-and-hold detection for QuickShape (happypaint `input/idle-hold.ts`,
//! v0.204.10): the pointer has been still for `hold_ms` of INPUT time.
//!
//! happypaint's bare timer could fire during a long frame, before the moves
//! queued behind the stall were dispatched, and snap a stroke still being
//! drawn. Here there is no timer: the canvas arms the hold with each move's
//! time, then polls once a frame *after* applying that frame's moves, so a
//! stall with moves queued behind it can never look like a hold. A stall with
//! nothing queued is a real hold and still fires.

/// One stroke's hold detector. The default is disarmed.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdleHold {
    hold_ms: f64,
    last_move: Option<f64>,
}

impl IdleHold {
    pub fn new(hold_ms: f64) -> Self {
        Self { hold_ms, last_move: None }
    }

    /// The pointer moved at `at` (ms, the move's own time where known).
    pub fn arm(&mut self, at: f64) {
        self.last_move = Some(at);
    }

    /// Stop waiting: the stroke ended, the pen dipped, the shape fired.
    pub fn clear(&mut self) {
        self.last_move = None;
    }

    /// Call once a frame, after that frame's moves were armed: true, once,
    /// when the pointer has been still for the hold time by `now`.
    pub fn poll(&mut self, now: f64) -> bool {
        match self.last_move {
            Some(t) if now - t >= self.hold_ms => {
                self.last_move = None;
                true
            }
            _ => false,
        }
    }

    /// How long until the hold could fire (for scheduling the next frame);
    /// `None` when not armed.
    pub fn due_in(&self, now: f64) -> Option<f64> {
        self.last_move.map(|t| (self.hold_ms - (now - t)).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cases from happypaint `idle-hold.test.ts`: time is in ms, and each
    // `poll` is a frame that runs after that frame's moves were `arm`ed.

    #[test]
    fn fires_once_still_for_the_hold_time() {
        let mut h = IdleHold::new(600.0);
        h.arm(0.0);
        assert!(!h.poll(599.0));
        assert!(h.poll(600.0));
        assert!(!h.poll(700.0), "once");
    }

    #[test]
    fn a_move_re_arms_so_steady_drawing_never_snaps() {
        let mut h = IdleHold::new(600.0);
        for i in 0..20 {
            let t = i as f64 * 100.0;
            h.arm(t);
            assert!(!h.poll(t));
        }
    }

    #[test]
    fn a_stall_with_moves_queued_behind_it_does_not_snap() {
        // A 900 ms frame mid-stroke: its queued moves (stamped 850) are
        // applied before the next poll.
        let mut h = IdleHold::new(600.0);
        h.arm(0.0);
        h.arm(850.0);
        assert!(!h.poll(900.0));
    }

    #[test]
    fn a_queued_move_counts_its_own_time() {
        // The last move was 450 ms into a 900 ms stall: the hold resolves
        // 150 ms after the stall, not a full 600 ms later.
        let mut h = IdleHold::new(600.0);
        h.arm(0.0);
        h.arm(450.0);
        assert!(!h.poll(900.0));
        assert_eq!(h.due_in(900.0), Some(150.0));
        assert!(h.poll(1050.0));
    }

    #[test]
    fn a_stall_with_nothing_queued_is_a_real_hold() {
        let mut h = IdleHold::new(600.0);
        h.arm(0.0);
        assert!(h.poll(900.0));
    }

    #[test]
    fn clear_cancels_the_hold() {
        let mut h = IdleHold::new(600.0);
        h.arm(0.0);
        h.clear();
        assert!(!h.poll(2000.0));
        assert_eq!(h.due_in(2000.0), None);
    }
}
