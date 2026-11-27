//! Input normalisation: per-device pressure (happypaint `input/pressure.ts`).

pub mod dip;
pub mod idle_hold;
pub mod pen_curve;
pub mod tablet;

pub use pen_curve::PenCurve;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Mouse,
    Touch,
    Pen,
}

const FLAT_EPS: f64 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Undecided,
    Flat,
    Hardware,
}

/// Per-stroke pressure source. Mouse and touch always use velocity-synthesised
/// pressure; a pen whose pressure reads as a flat 0.5 does too, until a real
/// value shows up. Coordinates are screen pixels, `t` in ms.
pub struct PressureProvider {
    kind: PointerKind,
    mode: Mode,
    last: Option<(f64, f64, f64)>,
    ema: f64,
    /// The user's pen pressure curve (Settings ▸ Brush), for hardware pressure.
    curve: PenCurve,
}

impl PressureProvider {
    pub fn new(kind: PointerKind) -> Self {
        Self::with_curve(kind, PenCurve::default())
    }

    /// A provider whose hardware pen pressure goes through `curve`.
    pub fn with_curve(kind: PointerKind, curve: PenCurve) -> Self {
        Self { kind, mode: Mode::Undecided, last: None, ema: 0.7, curve }
    }

    fn synth(&mut self, x: f64, y: f64, t: f64) -> f64 {
        let Some((lx, ly, lt)) = self.last else {
            self.last = Some((x, y, t));
            return self.ema;
        };
        let dt = (t - lt).max(1.0);
        let speed = (x - lx).hypot(y - ly) / dt;
        let target = (1.0 - speed * 0.18).clamp(0.25, 1.0);
        self.ema += (target - self.ema) * 0.25;
        self.last = Some((x, y, t));
        self.ema
    }

    pub fn pressure(&mut self, x: f64, y: f64, t: f64, raw: Option<f64>) -> f64 {
        if self.kind != PointerKind::Pen {
            return self.synth(x, y, t);
        }
        let raw = raw.unwrap_or(0.5);
        let flat = (raw - 0.5).abs() <= FLAT_EPS;
        self.mode = match self.mode {
            Mode::Undecided if flat => Mode::Flat,
            Mode::Undecided => Mode::Hardware,
            Mode::Flat if !flat => Mode::Hardware,
            m => m,
        };
        match self.mode {
            Mode::Hardware => self.curve.apply(raw),
            _ => self.synth(x, y, t),
        }
    }
}
