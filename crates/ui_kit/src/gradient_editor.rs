//! The Gradient tool's stop editor (happypaint `gradient-section.tsx`):
//! the gradient over a checker, a handle under each stop (tagged F or B
//! when it follows a swatch) and a diamond above each segment's midpoint.
//! Click the bar to add a stop, drag a handle (3 px dead zone) to move it,
//! double-click one to delete it (two always stay); drag a diamond to bend
//! the segment, double-click it to straighten. The caller owns the stops:
//! the editor reports what the user did.

use crate::theme::Tokens;
use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

/// A stop as the editor shows it: position, colour (straight sRGB 0..1),
/// opacity, midpoint of the segment to the next stop, and a tag letter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdStop {
    pub pos: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    pub mid: Option<f32>,
    pub tag: Option<char>,
}

/// What the user did; `commit` is false while a drag is still going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradEdit {
    Move {
        stop: usize,
        pos: f32,
        commit: bool,
    },
    /// The midpoint of the segment starting at `stop` (`None` = straight).
    Mid {
        stop: usize,
        mid: Option<f32>,
        commit: bool,
    },
    Add {
        pos: f32,
    },
    Delete {
        stop: usize,
    },
}

const MID_MIN: f32 = 0.05;
const MID_MAX: f32 = 0.95;
/// Under this gap a segment shows no midpoint diamond.
const MID_GAP: f32 = 0.02;

/// Where the colour is halfway, bent by a midpoint (`bendFraction`).
fn bend(f: f32, mid: Option<f32>) -> f32 {
    match mid {
        Some(m) if m != 0.5 && f > 0.0 && f < 1.0 => {
            let m = m.clamp(MID_MIN, MID_MAX);
            if f <= m { 0.5 * f / m } else { 0.5 + 0.5 * (f - m) / (1.0 - m) }
        }
        _ => f,
    }
}

/// The stops' order by position (indices into `stops`).
fn order(stops: &[EdStop]) -> Vec<usize> {
    let mut o: Vec<usize> = (0..stops.len()).collect();
    o.sort_by(|a, b| stops[*a].pos.total_cmp(&stops[*b].pos));
    o
}

/// Colour and opacity at `t` (`sampleStops`).
pub fn sample(stops: &[EdStop], t: f32) -> ([f32; 3], f32) {
    let o = order(stops);
    let (Some(&first), Some(&last)) = (o.first(), o.last()) else { return ([0.0; 3], 1.0) };
    let at = |i: usize| (stops[i].color, stops[i].opacity);
    if t <= stops[first].pos {
        return at(first);
    }
    if t >= stops[last].pos {
        return at(last);
    }
    for w in o.windows(2) {
        let (a, b) = (&stops[w[0]], &stops[w[1]]);
        if a.pos <= t && t <= b.pos {
            let f = bend(if b.pos == a.pos { 0.0 } else { (t - a.pos) / (b.pos - a.pos) }, a.mid);
            return (std::array::from_fn(|k| a.color[k] + (b.color[k] - a.color[k]) * f), a.opacity + (b.opacity - a.opacity) * f);
        }
    }
    at(last)
}

fn rgb(c: [f32; 3], a: f32) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(b(c[0]), b(c[1]), b(c[2]), b(a))
}

/// What a press grabbed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Stop(usize),
    Mid(usize),
}

/// The editor; `selected` follows the last stop pressed or added.
pub fn gradient_editor(ui: &mut Ui, stops: &[EdStop], selected: &mut usize, label: &str) -> (Response, Option<GradEdit>) {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().max(140.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click_and_drag());
    let bar = Rect::from_min_size(rect.min + vec2(7.0, 11.0), vec2(rect.width() - 14.0, 20.0));
    let x_of = |p: f32| bar.left() + bar.width() * p;
    let pos_of = |x: f32| ((x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0);
    let o = order(stops);
    // Midpoints: one per neighbouring pair wide enough, owned by the left stop.
    let mids: Vec<(usize, f32)> = o
        .windows(2)
        .filter(|w| stops[w[1]].pos - stops[w[0]].pos >= MID_GAP)
        .map(|w| {
            let (a, b) = (&stops[w[0]], &stops[w[1]]);
            (w[0], a.pos + a.mid.unwrap_or(0.5) * (b.pos - a.pos))
        })
        .collect();
    let hit = |p: Pos2| -> Option<Grab> {
        let near = |x: f32| (x - p.x).abs() <= 7.0;
        if p.y > bar.bottom() {
            o.iter().rev().copied().find(|&i| near(x_of(stops[i].pos))).map(Grab::Stop)
        } else if p.y < bar.top() {
            mids.iter().find(|(_, at)| near(x_of(*at))).map(|(i, _)| Grab::Mid(*i))
        } else {
            None
        }
    };
    let id = resp.id.with("grab");
    let mut edit = None;
    let origin = ui.input(|i| i.pointer.press_origin());
    let now = ui.input(|i| i.time);
    // A press grabs a handle; the drag past 3 px moves it.
    if resp.drag_started()
        && let Some(g) = origin.and_then(hit)
    {
        if let Grab::Stop(i) = g {
            *selected = i;
        }
        ui.data_mut(|d| d.insert_temp(id, (g, false)));
    }
    let grabbed = ui.data(|d| d.get_temp::<(Grab, bool)>(id));
    if let (Some((g, moved)), Some(p), Some(o0)) = (grabbed, resp.interact_pointer_pos(), origin)
        && resp.dragged()
    {
        let moved = moved || (p.x - o0.x).abs() > 3.0;
        ui.data_mut(|d| d.insert_temp(id, (g, moved)));
        if moved {
            edit = Some(match g {
                Grab::Stop(i) => GradEdit::Move { stop: i, pos: pos_of(p.x), commit: false },
                Grab::Mid(i) => {
                    let next = o.iter().position(|&k| k == i).and_then(|k| o.get(k + 1)).map_or(1.0, |&n| stops[n].pos);
                    let from = stops[i].pos;
                    let m = ((pos_of(p.x) - from) / (next - from).max(1e-6)).clamp(MID_MIN, MID_MAX);
                    GradEdit::Mid { stop: i, mid: Some(m), commit: false }
                }
            });
        }
    }
    if resp.drag_stopped() {
        if let Some((g, true)) = grabbed
            && let Some(p) = resp.interact_pointer_pos()
        {
            edit = Some(match g {
                Grab::Stop(i) => GradEdit::Move { stop: i, pos: pos_of(p.x), commit: true },
                Grab::Mid(i) => GradEdit::Mid { stop: i, mid: stops[i].mid, commit: true },
            });
            ui.data_mut(|d| d.insert_temp(id.with("dragged-at"), now));
        }
        ui.data_mut(|d| d.remove::<(Grab, bool)>(id));
    }
    if resp.double_clicked() {
        match resp.interact_pointer_pos().and_then(hit) {
            Some(Grab::Stop(i)) if stops.len() > 2 => {
                *selected = 0;
                edit = Some(GradEdit::Delete { stop: i });
            }
            Some(Grab::Mid(i)) => edit = Some(GradEdit::Mid { stop: i, mid: None, commit: true }),
            _ => {}
        }
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let recent = ui.data(|d| d.get_temp::<f64>(id.with("dragged-at"))).is_some_and(|at| now - at < 0.3);
        match hit(p) {
            Some(Grab::Stop(i)) => *selected = i,
            None if bar.contains(p) && !recent => edit = Some(GradEdit::Add { pos: pos_of(p.x) }),
            _ => {}
        }
    }

    // ── Drawing ──
    let painter = ui.painter_at(rect);
    crate::widgets::checker(&painter, bar, 5.0);
    let mut mesh = egui::Mesh::default();
    let n = 64;
    for k in 0..=n {
        let f = k as f32 / n as f32;
        let (c, a) = sample(stops, f);
        let x = x_of(f);
        mesh.colored_vertex(pos2(x, bar.top()), rgb(c, a));
        mesh.colored_vertex(pos2(x, bar.bottom()), rgb(c, a));
        if k > 0 {
            let v = 2 * k as u32;
            mesh.add_triangle(v - 2, v - 1, v);
            mesh.add_triangle(v - 1, v, v + 1);
        }
    }
    painter.add(mesh);
    painter.rect_stroke(bar, 2.0, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
    for (_, at) in &mids {
        let (x, y) = (x_of(*at), bar.top() - 5.0);
        let pts = vec![pos2(x, y - 4.0), pos2(x + 4.0, y), pos2(x, y + 4.0), pos2(x - 4.0, y)];
        painter.add(Shape::convex_polygon(pts, t.bg_elevated, Stroke::new(1.0, t.fg_muted)));
    }
    for &i in &o {
        let s = &stops[i];
        let (x, y) = (x_of(s.pos), bar.bottom() + 2.0);
        let pts = vec![pos2(x, y), pos2(x - 6.0, y + 9.0), pos2(x + 6.0, y + 9.0)];
        let stroke = if i == *selected { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.fg_muted) };
        painter.add(Shape::convex_polygon(pts, rgb(s.color, 1.0), stroke));
        if let Some(tag) = s.tag {
            painter.text(pos2(x, y + 11.0), egui::Align2::CENTER_TOP, tag, crate::theme::medium(8.5), t.fg_muted);
        }
    }
    let count = stops.len();
    let resp = resp.on_hover_text("Click to add a stop · drag to move · double-click to delete");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{label}: {count} stops")));
    (resp, edit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pos: f32, c: [f32; 3], o: f32, mid: Option<f32>) -> EdStop {
        EdStop { pos, color: c, opacity: o, mid, tag: None }
    }

    // happypaint gradient.test.ts: sampling with midpoints.
    #[test]
    fn samples_bend_at_the_midpoint() {
        let ramp = [s(0.0, [1.0, 0.0, 0.0], 1.0, None), s(0.5, [0.0, 1.0, 0.0], 0.5, None), s(1.0, [0.0, 0.0, 1.0], 0.0, None)];
        assert_eq!(sample(&ramp, 0.25), ([0.5, 0.5, 0.0], 0.75));
        let seg = [s(0.0, [0.0; 3], 0.0, Some(0.25)), s(1.0, [1.0; 3], 1.0, None)];
        assert!((sample(&seg, 0.25).1 - 0.5).abs() < 1e-6, "halfway at the midpoint");
        // Out of order in the list, still read in order.
        let back = [ramp[2], ramp[0], ramp[1]];
        assert_eq!(sample(&back, 0.25), ([0.5, 0.5, 0.0], 0.75));
    }
}
