//! The Blend If slider bar (happypaint `blend-if-bar.tsx`): a tone ramp
//! with a black and a white slider, each of which can be pulled apart into
//! two halves for a soft transition. Values are tones 0..=255 as
//! `[black low, black high, white low, white high]`.

use crate::theme::Tokens;
use egui::{Color32, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

/// Which slider a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Black,
    White,
}

/// Which part of the slider moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Both,
    Low,
    High,
}

/// Sort each slider's halves and keep black from passing white
/// (`clampOrder`).
pub fn clamp_order(r: [u8; 4]) -> [u8; 4] {
    let (b0, b1) = (r[0].min(r[1]), r[0].max(r[1]));
    let (w0, w1) = (r[2].min(r[3]), r[2].max(r[3]));
    [b0, b1.min(w0), w0.max(b1), w1]
}

/// Move one slider (or one half of it) to tone `v` (`moveHandle`). Moving a
/// whole split slider keeps its width.
pub fn move_handle(r: [u8; 4], h: Handle, v: u8, part: Part) -> [u8; 4] {
    let [b0, b1, w0, w1] = r;
    let out = match (h, part) {
        (Handle::Black, Part::Both) => {
            let d = b1 - b0;
            let lo = v.min(w0.saturating_sub(d));
            [lo, lo + d, w0, w1]
        }
        (Handle::White, Part::Both) => {
            let d = w1 - w0;
            let lo = v.clamp(b1.min(255 - d), 255 - d);
            [b0, b1, lo, lo + d]
        }
        (Handle::Black, Part::Low) => [v.min(b1), b1, w0, w1],
        (Handle::Black, Part::High) => [b0, v.max(b0), w0, w1],
        (Handle::White, Part::Low) => [b0, b1, v.min(w1), w1],
        (Handle::White, Part::High) => [b0, b1, w0, v.max(w0)],
    };
    clamp_order(out)
}

/// What a drag is holding: the slider, and the part once it is known
/// (`None`: an unsplit slider being split, waiting for the first move).
#[derive(Clone, Copy, Debug)]
struct Grab {
    handle: Handle,
    part: Option<Part>,
}

/// A tone bar from black to `tint`. Dragging moves the nearer slider; with
/// `split` on (or Alt held) an unsplit slider splits, the half chosen by the
/// first move. `label` names it for screen readers.
pub fn range_bar(ui: &mut Ui, range: &mut [u8; 4], tint: Color32, split: bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().max(120.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click_and_drag());
    let track = Rect::from_min_size(rect.min + vec2(7.0, 2.0), vec2(rect.width() - 14.0, 10.0));
    let x_of = |v: u8| track.left() + track.width() * f32::from(v) / 255.0;
    let tone = |x: f32| (((x - track.left()) / track.width()).clamp(0.0, 1.0) * 255.0).round() as u8;
    let id = resp.id.with("grab");
    // Pick by where the press began: by the time a drag registers the pointer has moved.
    if resp.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos())
    {
        let [b0, b1, w0, w1] = *range;
        let mid = |a: u8, b: u8| (x_of(a) + x_of(b)) / 2.0;
        let handle = if (p.x - mid(b0, b1)).abs() <= (p.x - mid(w0, w1)).abs() { Handle::Black } else { Handle::White };
        let (lo, hi) = match handle {
            Handle::Black => (b0, b1),
            Handle::White => (w0, w1),
        };
        let part = if lo != hi {
            Some(if (p.x - x_of(lo)).abs() <= (p.x - x_of(hi)).abs() { Part::Low } else { Part::High })
        } else if split || ui.input(|i| i.modifiers.alt) {
            None
        } else {
            Some(Part::Both)
        };
        ui.data_mut(|d| {
            d.insert_temp(id, Grab { handle, part });
            d.insert_temp(resp.id.with("keys"), handle);
        });
        resp.request_focus();
    }
    // Keys (`blend-if-bar.tsx`): ←/↓ and →/↑ move the slider last held (black
    // at first) by 1, or 10 with Shift, keeping a split slider's width.
    if resp.clicked() {
        resp.request_focus();
    }
    if resp.has_focus() {
        let filter = egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
        ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
        let step = ui.input(|i| {
            let k = if i.modifiers.shift { 10 } else { 1 };
            let up = i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp);
            let down = i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown);
            i32::from(up) * k - i32::from(down) * k
        });
        if step != 0 {
            let handle = ui.data(|d| d.get_temp::<Handle>(resp.id.with("keys"))).unwrap_or(Handle::Black);
            let lo = match handle {
                Handle::Black => range[0],
                Handle::White => range[2],
            };
            let v = (i32::from(lo) + step).clamp(0, 255) as u8;
            let next = move_handle(*range, handle, v, Part::Both);
            if next != *range {
                *range = next;
                resp.mark_changed();
            }
        }
    }
    if resp.dragged()
        && let (Some(p), Some(mut g)) = (resp.interact_pointer_pos(), ui.data(|d| d.get_temp::<Grab>(id)))
    {
        let dx = resp.drag_delta().x;
        if g.part.is_none() && dx != 0.0 {
            g.part = Some(if dx < 0.0 { Part::Low } else { Part::High });
            ui.data_mut(|d| d.insert_temp(id, g));
        }
        if let Some(part) = g.part {
            let next = move_handle(*range, g.handle, tone(p.x), part);
            if next != *range {
                *range = next;
                resp.mark_changed();
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Grab>(id));
    }
    let painter = ui.painter();
    let mut mesh = egui::Mesh::default();
    for (x, c) in [(track.left(), Color32::BLACK), (track.right(), tint)] {
        mesh.colored_vertex(pos2(x, track.top()), c);
        mesh.colored_vertex(pos2(x, track.bottom()), c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(mesh);
    painter.rect_stroke(track, 2.0, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
    // A triangle under each half; the halves of a split slider are drawn hollow.
    let marker = |x: f32, fill: Color32, hollow: bool| {
        let (y, s) = (track.bottom() + 2.0, 6.0);
        let pts = vec![pos2(x, y), pos2(x - s, y + s * 1.6), pos2(x + s, y + s * 1.6)];
        let stroke = Stroke::new(1.0, t.fg_muted);
        if hollow { Shape::convex_polygon(pts, t.bg, stroke) } else { Shape::convex_polygon(pts, fill, stroke) }
    };
    let [b0, b1, w0, w1] = *range;
    for (v, fill, hollow) in
        [(b0, Color32::BLACK, b0 != b1), (b1, Color32::BLACK, b0 != b1), (w0, Color32::WHITE, w0 != w1), (w1, Color32::WHITE, w0 != w1)]
    {
        painter.add(marker(x_of(v), fill, hollow));
    }
    let v = f64::from(b0);
    resp.widget_info(|| egui::WidgetInfo::slider(true, v, label));
    resp
}

/// The readout happypaint shows: `lo` or `lo/hi` per slider, "black · white".
pub fn range_readout(r: [u8; 4]) -> String {
    let half = |a: u8, b: u8| if a == b { a.to_string() } else { format!("{a}/{b}") };
    format!("{} · {}", half(r[0], r[1]), half(r[2], r[3]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_order_sorts_pairs_and_keeps_black_below_white() {
        assert_eq!(clamp_order([10, 5, 200, 100]), [5, 10, 100, 200]);
        assert_eq!(clamp_order([0, 150, 100, 255]), [0, 100, 150, 255]);
    }

    #[test]
    fn moving_a_whole_split_slider_keeps_its_width() {
        assert_eq!(move_handle([20, 40, 200, 255], Handle::Black, 100, Part::Both), [100, 120, 200, 255]);
        assert_eq!(move_handle([20, 40, 200, 255], Handle::Black, 250, Part::Both), [180, 200, 200, 255]);
        assert_eq!(move_handle([0, 0, 200, 220], Handle::White, 250, Part::Both), [0, 0, 235, 255]);
        assert_eq!(move_handle([0, 50, 200, 220], Handle::White, 10, Part::Both), [0, 50, 50, 70]);
    }

    #[test]
    fn a_half_stays_on_its_side() {
        assert_eq!(move_handle([0, 0, 255, 255], Handle::White, 180, Part::Low), [0, 0, 180, 255]);
        assert_eq!(move_handle([0, 0, 180, 255], Handle::White, 100, Part::High), [0, 0, 180, 180]);
        assert_eq!(move_handle([30, 60, 255, 255], Handle::Black, 90, Part::Low), [60, 60, 255, 255]);
        assert_eq!(range_readout([30, 60, 255, 255]), "30/60 · 255");
    }
}
