//! Help (happypaint `components/help-dialog`): the shortcut and gesture
//! reference, and a guide to the tool bar, panels and features that need
//! more than a key to explain.

use crate::App;
use egui::{Id, RichText, Ui};
use onepaint_ui_kit::{Tokens, theme, widgets};

/// Guide topics: a title and (gesture or control, what it does) rows.
pub const GUIDE: &[(&str, &[(&str, &str)])] = &[
    (
        "Tool bar",
        &[
            ("Grip (dotted handle)", "Drag to move the bar anywhere"),
            ("Double-click the grip", "Switch between a horizontal bar and a vertical one"),
            ("Drop it at a window edge", "Docks it as a full-height rail; the canvas makes room. Drag the grip again to tear it off"),
            ("Its trailing edge", "Drag to set how many button columns it has; double-click for the automatic width"),
            ("Click the active tool again", "Opens that tool's options"),
            ("Narrow window", "Below 900 px the bar becomes a strip above the status bar, and docked panels open as popovers"),
        ],
    ),
    (
        "Status bar",
        &[
            ("Left", "Saved state, the tool, brush and size, your colour, the layer and the cursor position"),
            ("Colour swatch", "Drag it onto the canvas for ColorDrop (below)"),
            ("Fill tool", "Threshold and Grow sliders appear next to the colour"),
            ("Document", "Size, DPI and bit depth; click for the paper and document settings"),
            ("Dial and degrees", "Drag the dial to rotate the view; click it, or double-click the degrees, to reset"),
            ("− 100% +", "Zoom out, back to 100%, zoom in"),
            ("Mirror buttons", "Vertical and horizontal symmetry; both on gives four-way"),
            (
                "Crosshair (symmetry on)",
                "Move axis: drag on the canvas to place the mirror centre (also Shift+Y); click it again when done",
            ),
            ("Curve (perspective on)", "Edit the vanishing points on the canvas (Shift+P)"),
            ("Bucket / contrast", "Fill mode (brush strokes fill their outline, F) and the grayscale value check (Ctrl+Y)"),
        ],
    ),
    (
        "ColorDrop",
        &[
            ("Drag the status-bar swatch onto the canvas", "Fills the area under the pointer with that colour, live, as you move"),
            ("Hold still for a moment", "Locks the spot; now slide left or right to lower or raise the threshold (shown at the top)"),
            ("Release", "Keeps the fill, as one undo step"),
            ("Esc, or leave the canvas", "Cancels it"),
            ("What it fills", "The active layer, like the bucket: inside the selection, respecting alpha lock"),
        ],
    ),
    (
        "Panels & docking",
        &[
            ("Grip at the top of a popover", "Layers, Brushes, Colour, Guides and tool options: drag it to float the panel"),
            ("Drop a panel at a window edge", "Docks it to that side; the drop height picks its slot"),
            ("Drop it on a docked panel's header", "Joins that side as tabs"),
            (
                "Drop it on a floating panel's title",
                "The two share one floating window with a tab each; drag a tab out to split them again",
            ),
            ("Drag a docked header or a floating title", "Tears the panel off again"),
            ("Splitter between docked panels", "Drag to share the height; drag a dock's inner edge to set its width"),
            ("Chevron", "Collapses a docked panel to its header, or folds a floating one to its title (double-click works too)"),
            ("Stack / tabs icon", "Shows a side's panels as tabs, or stacks them again"),
            ("Floating window edges", "Drag to resize"),
            ("Menu ▸ Panels", "Opens Align, Properties, Tool options and Palettes, and resets the layout"),
            ("Saved", "Where everything lives is remembered between sessions"),
        ],
    ),
    (
        "Panel (manga)",
        &[
            ("Pick the Panel tool", "Its options open: ready-made page layouts (1×2 … 4×2), gutter, border and page margin"),
            ("Drag on bare page", "Draws a panel: a group with an empty Art layer inside, plus its border"),
            ("Drag inside a panel", "Cuts it in two along the line; the panel keeps its art and one half"),
            ("Drag a corner handle", "Reshapes that corner; the handles show whenever the tool is active"),
            ("Drag the frame line", "Moves the whole frame. The art stays put, so this re-crops what you drew"),
            ("While dragging", "A new frame previews as a rectangle, a cut as a line across the panel"),
            ("A very short drag", "Does nothing, so a stray tap never leaves a stray frame"),
            ("Art is clipped", "Anything painted on a panel's layers stops at its frame"),
            ("Gutter and Border", "Apply to every panel on the page at once; one undo step per slider drag"),
            ("Borders", "One editable path layer for the page, redrawn whenever panels change"),
            ("Undo", "One step per frame, cut, reshape or move"),
        ],
    ),
    (
        "Manga & comics",
        &[
            ("Manga brushes", "A Manga category in the brush library: G-pen, Maru pen, Millipen, Tone Scraper, Tone Fade"),
            ("Screentone", "Adjustments ▸ Pixelate ▸ Screentone, as a live layer: LPI, density, angle and dot shape"),
            ("Focus / speed lines", "Adjustments ▸ Effect lines: drawn with your current brush as editable path layers"),
            ("Balloons", "Text tool: speech, rounded, shout and thought shapes with a tail"),
            ("Books", "Open several pages as tabs and save them together as a .happypaint-book"),
        ],
    ),
    (
        "Properties",
        &[
            ("What it shows", "The selected adjustment or fill layer's settings"),
            ("Where it lives", "Docked on the right; it opens on its own when you add or pick an adjustment layer"),
            ("Other layers", "It says so; pick an adjustment layer to edit it"),
        ],
    ),
    (
        "Align & distribute",
        &[
            ("Open it", "Menu ▸ Panels ▸ align icon; it floats, docks and tabs like any panel"),
            ("Pick layers", "In the Layers panel: Ctrl/⌘-click adds or removes a layer, Shift-click picks a range, a plain click clears"),
            ("What moves", "The picked layers; hidden and locked ones sit it out, a picked group means its children"),
            ("Measured by", "Each layer's artwork: the tight box around its painted pixels"),
            ("Align (six buttons)", "Left, centre, right, then top, middle, bottom"),
            ("Distribute centres", "Spaces the centres evenly, keeping the two outermost layers (3+ layers)"),
            ("Even gaps", "Equal space between the layers instead; differs when they are different sizes (3+ layers)"),
            ("Align to", "The canvas, or the selection; with no selection, the picked layers themselves"),
            ("Text and paths", "Stay editable; one undo step for the whole move"),
        ],
    ),
];

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Shortcuts,
    Guide,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(600.0);
    let id = Id::new("help-tab");
    let mut tab: Tab = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Help").font(theme::semibold(15.0)).color(t.fg));
        ui.add_space(12.0);
        if let Some(i) = crate::chrome::segmented(ui, &["Shortcuts", "Guide"], usize::from(tab == Tab::Guide)) {
            tab = if i == 1 { Tab::Guide } else { Tab::Shortcuts };
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, tab));
    ui.add_space(6.0);
    match tab {
        Tab::Shortcuts => shortcuts(ui, &app.settings.keybindings),
        Tab::Guide => guide(ui),
    }
}

fn guide(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() - 140.0).show(ui, |ui| {
        for (title, rows) in GUIDE {
            widgets::section_label(ui, title);
            egui::Grid::new(("guide", *title)).num_columns(2).spacing([14.0, 5.0]).min_col_width(170.0).show(ui, |ui| {
                for (what, how) in rows.iter() {
                    ui.add(egui::Label::new(RichText::new(*what).font(theme::medium(11.5)).color(t.fg)).wrap());
                    ui.add(egui::Label::new(RichText::new(*how).color(t.fg_muted)).wrap());
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
        }
    });
}

fn shortcuts(ui: &mut Ui, keys: &crate::keymap::Overrides) {
    let t = Tokens::get(ui.ctx());
    let groups: [(&str, &[(&str, &str)]); 6] = [
        (
            "Tools",
            &[
                ("B / E", "Brush / eraser"),
                ("Shift+B", "Mixer brush"),
                ("S", "Smudge"),
                ("G", "Fill"),
                ("I or Alt-click", "Eyedropper"),
                ("V or Ctrl+T", "Move / transform"),
                ("H or hold Space", "Pan"),
                ("T / P / U", "Text / pen / shape"),
                ("M / L / W", "Rectangle / lasso / wand select"),
            ],
        ),
        (
            "Brush & colour",
            &[
                ("[ / ]", "Brush size"),
                ("X", "Swap colours"),
                ("D", "Black & white"),
                ("Alt+Backspace", "Fill with colour"),
                ("Ctrl+Backspace", "Fill with background"),
            ],
        ),
        (
            "View",
            &[
                ("= / −", "Zoom in / out"),
                ("0 / 1", "Fit / 100%"),
                ("Ctrl+[ / ]", "Rotate 15°"),
                ("R-drag, 5", "Rotate / reset rotation"),
                ("Y, Shift+Y", "Symmetry, move axis"),
                ("Shift+R / G / P / D", "Rulers, guides, perspective, measure"),
                ("Alt+L / Alt+B / Alt+R", "Layers / brushes / reference"),
                ("Ctrl+Y", "Grayscale value check"),
                ("Tab", "Hide the interface"),
            ],
        ),
        (
            "Touch",
            &[
                ("Pinch / two fingers", "Zoom, rotate, pan"),
                ("Two-finger tap", "Undo"),
                ("Three-finger tap", "Redo"),
                ("Three-finger swipe down", "Copy & paste"),
                ("Four-finger tap", "Hide the interface"),
            ],
        ),
        (
            "Animation",
            &[
                ("Shift+A", "Animation mode"),
                ("K", "Play / pause"),
                (", / .", "Previous / next frame"),
                ("< / >", "First / last frame"),
                ("N", "Add frame"),
                ("O", "Onion skins"),
            ],
        ),
        (
            "Edit",
            &[
                ("Ctrl+Z / Ctrl+Shift+Z", "Undo / redo"),
                ("Ctrl+C / X / V", "Copy / cut / paste"),
                ("Ctrl+J", "Layer via copy"),
                ("Ctrl+A / D / Shift+I", "Select all / deselect / invert"),
                ("Ctrl+S / O / N", "Save / open / new"),
                ("Ctrl+Shift+E", "Export"),
            ],
        ),
    ];
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() - 120.0).show(ui, |ui| {
        ui.columns(2, |cols| {
            for (i, (title, rows)) in groups.iter().enumerate() {
                let ui = &mut cols[usize::from(i >= 3)];
                ui.label(RichText::new(*title).font(theme::semibold(12.5)).color(t.fg));
                egui::Grid::new(*title).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (k, what) in rows.iter() {
                        ui.label(RichText::new(*k).font(theme::mono(11.0)).color(t.accent));
                        ui.label(RichText::new(*what).color(t.fg_muted));
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
            }
            // The list above shows the defaults; changed shortcuts are listed here.
            if !keys.is_empty() {
                let ui = &mut cols[1];
                ui.label(RichText::new("Your shortcuts").font(theme::semibold(12.5)).color(t.fg));
                egui::Grid::new("your-keys").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (id, combo) in keys {
                        let Some(a) = crate::keymap::Action::from_id(id) else { continue };
                        ui.label(RichText::new(crate::keymap::format_combo(combo)).font(theme::mono(11.0)).color(t.accent));
                        ui.label(RichText::new(a.label()).color(t.fg_muted));
                        ui.end_row();
                    }
                });
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::GUIDE;

    #[test]
    fn the_guide_covers_the_panel_tool_and_docking() {
        let titles: Vec<&str> = GUIDE.iter().map(|(t, _)| *t).collect();
        for want in ["Panel (manga)", "Panels & docking", "Align & distribute", "ColorDrop", "Status bar", "Tool bar"] {
            assert!(titles.contains(&want), "{want}");
        }
        assert!(GUIDE.iter().all(|(_, rows)| !rows.is_empty()));
    }
}
