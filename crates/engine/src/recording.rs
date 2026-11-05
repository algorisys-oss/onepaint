//! Session recording, replay and timelapse (spec 26): a log of top-level
//! commands and stroke input against a start snapshot.

use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum RecEvent {
    Cmd {
        t: f64,
        cmd: Box<Command>,
    },
    Begin {
        t: f64,
        p: StrokePoint,
        /// The canvas zoom at stroke start (the stabilizer's string is
        /// measured on screen). Absent in older recordings: 100%.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zoom: Option<f64>,
        /// A Shift+click line: no smoothing, stabilizer or taper.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        straight: bool,
    },
    Extend {
        t: f64,
        pts: Vec<StrokePoint>,
    },
    End {
        t: f64,
    },
    Cancel {
        t: f64,
    },
    Tick {
        t: f64,
        now: f64,
    },
    /// Wet simulation steps run this frame (spec 35 B).
    WetSteps {
        t: f64,
        steps: usize,
    },
    /// A command from a happypaint recording (spec 62).
    Happy {
        t: f64,
        name: String,
        args: serde_json::Value,
    },
}

impl RecEvent {
    /// Milliseconds since the recording started.
    pub fn t(&self) -> f64 {
        match self {
            RecEvent::Cmd { t, .. }
            | RecEvent::Begin { t, .. }
            | RecEvent::Extend { t, .. }
            | RecEvent::End { t }
            | RecEvent::Cancel { t }
            | RecEvent::Tick { t, .. }
            | RecEvent::WetSteps { t, .. }
            | RecEvent::Happy { t, .. } => *t,
        }
    }
}

/// Session state outside the document that a replay must start from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartState {
    pub seed: u32,
    pub tool: Tool,
    pub brush: BrushPreset,
    pub eraser: BrushPreset,
    pub color: Rgba,
    pub active: NodeId,
    pub feather: f32,
    pub paint: PaintSettings,
    /// The selection as saved-selection run lengths.
    pub selection: Option<String>,
}

/// Every layer's pixel, mask and height tiles as raw f16 bits, deflated: the
/// `.happypaint` snapshot quantises pixels, and a replay must be exact.
fn dump_tiles(doc: &Document) -> Vec<u8> {
    use std::io::Write;
    let mut raw = Vec::new();
    let mut put = |id: &str, kind: u8, s: &Surface| {
        for ((tx, ty), t) in s.tiles() {
            raw.extend((id.len() as u16).to_le_bytes());
            raw.extend(id.as_bytes());
            raw.push(kind);
            raw.extend(tx.to_le_bytes());
            raw.extend(ty.to_le_bytes());
            raw.extend(t.f16_data().iter().flat_map(|v| v.to_bits().to_le_bytes()));
        }
    };
    for id in doc.layer_ids() {
        if let Some(l) = doc.layer(&id) {
            put(&id, 0, &l.pixels);
            if let Some(m) = &l.mask {
                put(&id, 1, m);
            }
            if let Some(h) = &l.height {
                put(&id, 2, h);
            }
        }
    }
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = enc.write_all(&raw);
    enc.finish().unwrap_or_default()
}

fn load_tiles(doc: &mut Document, bytes: &[u8]) -> Result<(), EngineError> {
    use std::io::Read;
    let mut raw = Vec::new();
    flate2::read::DeflateDecoder::new(bytes).read_to_end(&mut raw).map_err(|e| EngineError::Format(e.to_string()))?;
    let bad = || EngineError::Format("recording: bad tiles".into());
    let n = TILE_PX * 4 * 2;
    // Start from empty surfaces so layers keep exactly the recorded tiles.
    for id in doc.layer_ids() {
        if let Some(l) = doc.layer_mut(&id) {
            l.pixels = Surface::new();
            if let Some(m) = l.mask.as_mut() {
                *m = Surface::new();
            }
            l.height = None;
        }
    }
    let mut p = 0;
    while p < raw.len() {
        let len = u16::from_le_bytes(raw.get(p..p + 2).ok_or_else(bad)?.try_into().map_err(|_| bad())?) as usize;
        let id = std::str::from_utf8(raw.get(p + 2..p + 2 + len).ok_or_else(bad)?).map_err(|_| bad())?.to_string();
        p += 2 + len;
        let kind = *raw.get(p).ok_or_else(bad)?;
        let tx = i32::from_le_bytes(raw.get(p + 1..p + 5).ok_or_else(bad)?.try_into().map_err(|_| bad())?);
        let ty = i32::from_le_bytes(raw.get(p + 5..p + 9).ok_or_else(bad)?.try_into().map_err(|_| bad())?);
        let data = raw.get(p + 9..p + 9 + n).ok_or_else(bad)?;
        p += 9 + n;
        if data.len() != onepaint_raster::TILE_PX * 8 {
            return Err(bad());
        }
        let tile = Tile::from_f16(data.chunks_exact(2).map(|b| half::f16::from_bits(u16::from_le_bytes([b[0], b[1]]))).collect());
        if let Some(l) = doc.layer_mut(&id) {
            let target = match kind {
                1 => l.mask.get_or_insert_with(Surface::new),
                2 => l.height.get_or_insert_with(Surface::new),
                _ => &mut l.pixels,
            };
            target.set_tile(tx, ty, Some(Arc::new(tile)));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    /// The document when recording started (`.happypaint` bytes).
    pub start: Vec<u8>,
    /// Lossless pixels for the start document (see `dump_tiles`).
    pub tiles: Vec<u8>,
    pub state: StartState,
    pub events: Vec<RecEvent>,
}

impl Recording {
    pub fn to_bytes(&self) -> Result<Vec<u8>, EngineError> {
        let err = |e: serde_json::Error| EngineError::Format(e.to_string());
        let events = serde_json::to_vec(&self.events).map_err(err)?;
        let state = serde_json::to_vec(&self.state).map_err(err)?;
        Ok(onepaint_formats::zip::write_zip(&[
            ("start.happypaint", &self.start),
            ("tiles.bin", &self.tiles),
            ("state.json", &state),
            ("events.json", &events),
        ]))
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self, EngineError> {
        let files = onepaint_formats::zip::read_zip(b)?;
        let get =
            |n: &str| files.iter().find(|f| f.0 == n).map(|f| f.1.clone()).ok_or_else(|| EngineError::Format(format!("recording: no {n}")));
        let err = |e: serde_json::Error| EngineError::Format(e.to_string());
        let events = serde_json::from_slice(&get("events.json")?).map_err(err)?;
        let state = serde_json::from_slice(&get("state.json")?).map_err(err)?;
        Ok(Self { start: get("start.happypaint")?, tiles: get("tiles.bin")?, state, events })
    }
}

pub(crate) struct Recorder {
    started: std::time::Instant,
    start: Vec<u8>,
    tiles: Vec<u8>,
    state: StartState,
    events: Vec<RecEvent>,
}

impl Session {
    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    pub fn start_recording(&mut self) -> Result<(), EngineError> {
        let start = self.save_happypaint()?;
        let state = StartState {
            seed: self.seed,
            tool: self.tool,
            brush: self.brush.clone(),
            eraser: self.eraser.clone(),
            color: self.color,
            active: self.active.clone(),
            feather: self.feather,
            paint: self.paint,
            selection: self.selection.as_ref().and_then(|m| onepaint_selection::saved::encode_rle(m.data())),
        };
        let tiles = dump_tiles(&self.doc);
        self.recorder = Some(Recorder { started: std::time::Instant::now(), start, tiles, state, events: Vec::new() });
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Option<Recording> {
        self.recorder.take().map(|r| Recording { start: r.start, tiles: r.tiles, state: r.state, events: r.events })
    }

    fn restore_state(&mut self, s: &StartState) {
        (self.seed, self.tool, self.brush, self.eraser, self.color) = (s.seed, s.tool, s.brush.clone(), s.eraser.clone(), s.color);
        if self.doc.find(&s.active).is_some() {
            self.active = s.active.clone();
        }
        (self.feather, self.paint) = (s.feather, s.paint);
        let (w, h) = (self.doc.width, self.doc.height);
        let m = s
            .selection
            .as_deref()
            .and_then(|r| onepaint_selection::saved::decode_rle(r, w, h))
            .map(|d| Arc::new(onepaint_selection::Mask::from_vec(w, h, d)));
        self.set_selection_raw(m);
    }

    pub(crate) fn rec_time(&self) -> f64 {
        self.recorder.as_ref().map_or(0.0, |r| r.started.elapsed().as_secs_f64() * 1000.0)
    }

    /// Log an event if recording and not inside another command.
    pub(crate) fn rec(&mut self, ev: impl FnOnce(f64) -> RecEvent) {
        if self.rec_depth > 0 {
            return;
        }
        let t = self.rec_time();
        if let Some(r) = self.recorder.as_mut() {
            r.events.push(ev(t));
        }
    }

    /// Apply one recorded event (replay).
    pub fn apply_event(&mut self, ev: &RecEvent) {
        match ev {
            RecEvent::Cmd { cmd, .. } => {
                let _ = self.execute((**cmd).clone());
            }
            RecEvent::Begin { p, zoom, straight, .. } => {
                let live = self.prefs.view_zoom;
                self.prefs.view_zoom = zoom.unwrap_or(1.0);
                self.straight = *straight;
                self.stroke_begin(*p);
                self.straight = false;
                self.prefs.view_zoom = live;
            }
            RecEvent::Extend { pts, .. } => self.stroke_extend(pts),
            RecEvent::End { .. } => self.stroke_end(),
            RecEvent::Cancel { .. } => self.stroke_cancel(),
            RecEvent::Tick { now, .. } => self.stroke_tick(*now),
            RecEvent::WetSteps { steps, .. } => self.wet_steps(*steps),
            RecEvent::Happy { name, args, .. } => {
                self.apply_happy(name, args);
            }
        }
    }
}

/// Replays a recording into a fresh session.
pub struct Replay {
    pub session: Session,
    events: Vec<RecEvent>,
    pos: usize,
}

impl Replay {
    pub fn new(rec: &Recording) -> Result<Self, EngineError> {
        let mut doc = onepaint_formats::read_happypaint(&rec.start)?;
        load_tiles(&mut doc, &rec.tiles)?;
        let mut session = Session::from_document(doc);
        session.restore_state(&rec.state);
        Ok(Self { session, events: rec.events.clone(), pos: 0 })
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub(crate) fn from_parts(session: Session, events: Vec<RecEvent>) -> Self {
        Self { session, events, pos: 0 }
    }

    /// The start session and the events, for replaying on a live canvas.
    pub fn into_parts(self) -> (Session, Vec<RecEvent>) {
        (self.session, self.events)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn done(&self) -> bool {
        self.pos >= self.events.len()
    }

    /// Apply the next `n` events.
    pub fn step(&mut self, n: usize) {
        let end = (self.pos + n).min(self.events.len());
        for i in self.pos..end {
            let ev = self.events[i].clone();
            self.session.apply_event(&ev);
        }
        self.pos = end;
    }

    /// A timelapse GIF: up to 240 evenly spaced frames fitted in 720 px, the
    /// last held for a second.
    pub fn timelapse_gif(mut self) -> Result<Vec<u8>, EngineError> {
        let opts = |s: &Session| {
            let d = s.doc();
            let scale = (720.0 / d.width.max(d.height) as f64).min(1.0);
            onepaint_formats::export::ExportOptions { scale, ..Default::default() }
        };
        let n = self.len();
        let frames = n.clamp(1, 240);
        let mut out = Vec::new();
        let mut size = (0, 0);
        let mut grab = |s: &Session, out: &mut Vec<Vec<u8>>| {
            let d = s.doc();
            let o = opts(s);
            let (w, h) = onepaint_formats::export::output_size(d.width, d.height, o.scale);
            size = (w, h);
            let flat = s.flatten_linear();
            out.push(scaled_rgba(&flat, d.width, d.height, w, h));
        };
        for f in 1..=frames {
            let target = n * f / frames;
            self.step(target - self.pos);
            grab(&self.session, &mut out);
        }
        let last = out.last().cloned().unwrap_or_default();
        out.extend(std::iter::repeat_n(last, 12));
        Ok(onepaint_formats::encode_gif(size.0, size.1, &out, 12.0)?)
    }
}

impl Replay {
    /// A timelapse MP4 (happypaint's video timelapse): 30 fps, fitted in
    /// 1280 px (even sizes), 3–20 s mapped onto the time up to the last
    /// event, then the finished picture.
    pub fn timelapse_mp4(mut self) -> Result<Vec<u8>, EngineError> {
        const FPS: usize = 30;
        let d = self.session.doc();
        let (w, h) = (d.width, d.height);
        let k = (1280.0 / w.max(h) as f64).min(1.0);
        let even = |n: u32| ((n as f64 * k / 2.0).round() as u32 * 2).max(2);
        let (ow, oh) = (even(w), even(h));
        let span = self.events.last().map_or(0.0, RecEvent::t);
        let secs = ((span / 1000.0).round() as usize).clamp(3, 20);
        let total = (secs * FPS).max(2);
        let mut video = onepaint_formats::video::Mp4Writer::new(ow, oh, FPS as f64)?;
        for f in 0..total {
            let t = f as f64 / (total - 1) as f64 * span;
            let upto = if f + 1 == total {
                self.events.len()
            } else {
                self.events[self.pos..].iter().take_while(|e| e.t() <= t).count() + self.pos
            };
            self.step(upto - self.pos);
            let d = self.session.doc();
            video.push(&scaled_rgba(&self.session.flatten_linear(), d.width, d.height, ow, oh))?;
        }
        Ok(video.finish()?)
    }
}

/// Straight sRGB RGBA8 at (w, h), from premultiplied linear (white behind).
fn scaled_rgba(flat: &[f32], sw: u32, sh: u32, w: u32, h: u32) -> Vec<u8> {
    onepaint_formats::export::flat_to_rgba8(flat, sw, sh, w, h, true)
}
