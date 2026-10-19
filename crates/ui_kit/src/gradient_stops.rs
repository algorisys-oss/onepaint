//! A gradient's colour stops (happypaint `gradient-stops.tsx`): a bar
//! showing the gradient with a handle under each stop. Click the bar to add
//! a stop (coloured as the gradient is there), drag a handle to move it,
//! double-click one to delete it (two always stay). Stops keep their order
//! in the list; only the drawing sorts them.

use crate::theme::Tokens;
use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

/// One stop: where (0..=1) and its colour (0..=1 RGB).
pub type Stop = (f32, [f32; 3]);

/// The gradient's colour at `pos`, linear between the stops around it
/// (`colorAt`).
pub fn colour_at(stops: &[Stop], pos: f32) -> [f32; 3] {
    let mut s: Vec<Stop> = stops.to_vec();
    s.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (Some(first), Some(last)) = (s.first(), s.last()) else { return [0.0; 3] };
    if pos <= first.0 {
        return first.1;
    }
    if pos >= last.0 {
        return last.1;
    }
    for w in s.windows(2) {
        let (a, b) = (w[0], w[1]);
        if pos <= b.0 {
            let t = if b.0 > a.0 { (pos - a.0) / (b.0 - a.0) } else { 0.0 };
            return std::array::from_fn(|k| a.1[k] + (b.1[k] - a.1[k]) * t);
        }
    }
    last.1
}

/// Add a stop at `pos` coloured as the gradient is there; its index.
pub fn add_stop(stops: &mut Vec<Stop>, pos: f32) -> usize {
    let pos = pos.clamp(0.0, 1.0);
    stops.push((pos, colour_at(stops, pos)));
    stops.len() - 1
}

/// Delete stop `i`, keeping at least two; whether it went.
pub fn delete_stop(stops: &mut Vec<Stop>, i: usize) -> bool {
    if stops.len() <= 2 || i >= stops.len() {
        return false;
    }
    stops.remove(i);
    true
}

fn rgb(c: [f32; 3]) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(b(c[0]), b(c[1]), b(c[2]))
}

/// The bar. `selected` is the stop whose colour the caller edits; a press
/// on a handle selects it and a new stop is selected. Marked changed when
/// the stops change.
pub fn gradient_stops(ui: &mut Ui, stops: &mut Vec<Stop>, selected: &mut usize, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().max(120.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click_and_drag());
    let bar = Rect::from_min_size(rect.min + vec2(7.0, 2.0), vec2(rect.width() - 14.0, 18.0));
    let x_of = |p: f32| bar.left() + bar.width() * p;
    let pos_of = |x: f32| ((x - bar.left()) / bar.width()).clamp(0.0, 1.0);
    let handle_at = |stops: &[Stop], p: Pos2| {
        (p.y > bar.bottom())
            .then(|| {
                stops
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i, (x_of(s.0) - p.x).abs()))
                    .filter(|(_, d)| *d <= 8.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
            })
            .flatten()
    };
    let id = resp.id.with("grab");
    let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
    if resp.double_clicked()
        && let Some(i) = origin.and_then(|p| handle_at(stops, p))
    {
        if delete_stop(stops, i) {
            *selected = 0;
            resp.mark_changed();
        }
    } else if (resp.drag_started() || resp.clicked())
        && let Some(p) = origin
    {
        match handle_at(stops, p) {
            Some(i) => {
                *selected = i;
                ui.data_mut(|d| d.insert_temp(id, i));
            }
            None if resp.clicked() && bar.contains(p) => {
                *selected = add_stop(stops, pos_of(p.x));
                resp.mark_changed();
            }
            None => {}
        }
    }
    if resp.dragged()
        && let (Some(p), Some(i)) = (resp.interact_pointer_pos(), ui.data(|d| d.get_temp::<usize>(id)))
        && i < stops.len()
    {
        let v = pos_of(p.x);
        if stops[i].0 != v {
            stops[i].0 = v;
            resp.mark_changed();
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<usize>(id));
    }
    let painter = ui.painter();
    let mut sorted: Vec<Stop> = stops.clone();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut mesh = egui::Mesh::default();
    let ends = [(0.0, colour_at(stops, 0.0))].into_iter().chain(sorted.iter().copied()).chain([(1.0, colour_at(stops, 1.0))]);
    for (k, (p, c)) in ends.enumerate() {
        mesh.colored_vertex(pos2(x_of(p), bar.top()), rgb(c));
        mesh.colored_vertex(pos2(x_of(p), bar.bottom()), rgb(c));
        if k > 0 {
            let v = 2 * k as u32;
            mesh.add_triangle(v - 2, v - 1, v);
            mesh.add_triangle(v - 1, v, v + 1);
        }
    }
    painter.add(mesh);
    painter.rect_stroke(bar, 2.0, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
    for (i, s) in stops.iter().enumerate() {
        let (x, y, k) = (x_of(s.0), bar.bottom() + 2.0, 6.0);
        let pts = vec![pos2(x, y), pos2(x - k, y + k * 1.6), pos2(x + k, y + k * 1.6)];
        let stroke = if i == *selected { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.fg_muted) };
        painter.add(Shape::convex_polygon(pts, rgb(s.1), stroke));
    }
    let n = stops.len() as f64;
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{label}: {n} stops")));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    const BW: [Stop; 2] = [(0.0, [0.0; 3]), (1.0, [1.0; 3])];

    #[test]
    fn a_new_stop_takes_the_gradients_colour_there() {
        let mut s = BW.to_vec();
        let i = add_stop(&mut s, 0.25);
        assert_eq!(i, 2);
        assert_eq!(s[2], (0.25, [0.25; 3]));
        // Out of order in the list, still read in order.
        assert_eq!(colour_at(&s, 0.125), [0.125; 3]);
        assert_eq!(colour_at(&s, -1.0), [0.0; 3]);
        assert_eq!(colour_at(&s, 2.0), [1.0; 3]);
    }

    #[test]
    fn two_stops_always_stay() {
        let mut s = BW.to_vec();
        assert!(!delete_stop(&mut s, 0));
        add_stop(&mut s, 0.5);
        assert!(delete_stop(&mut s, 0));
        assert_eq!(s, vec![(1.0, [1.0; 3]), (0.5, [0.5; 3])]);
    }
}
