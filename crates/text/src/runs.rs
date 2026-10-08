//! Rich-text run edits (spec 28; happypaint `docs/rich-text-spec.md`,
//! `utils/rich-text-html.ts` and the selection toolbar in
//! `components/canvas/paint-canvas.tsx:358-460`).
//!
//! Ranges are in chars. Runs store only the fields that differ from the
//! object's base style. When every char matches the base, `runs` is `None`.

use crate::{TextRun, TextSpec};
use std::ops::Range;

/// One selection-toolbar command.
#[derive(Clone, Debug, PartialEq)]
pub enum StyleEdit {
    /// Toggle: off when every char in the range is already bold, else on.
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Color(String),
    Font(String),
    /// Multiply each char's own size (A− 1/1.1, A+ 1.1), 0.01 px, ≥ 2 px.
    ScaleSize(f64),
}

/// The style common to a range (for the toolbar state).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RangeStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    /// `None` when mixed.
    pub color: Option<String>,
    pub size: Option<f64>,
    pub family: Option<String>,
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn byte_at(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map_or(s.len(), |(b, _)| b)
}

/// `#rgb` / `#RRGGBB` → `#rrggbb`; anything else is lowercased.
pub fn normalize_color(c: &str) -> String {
    let s = c.trim().to_ascii_lowercase();
    let hex = s.strip_prefix('#').unwrap_or("");
    if hex.len() == 3 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return format!("#{}", hex.chars().flat_map(|c| [c, c]).collect::<String>());
    }
    s
}

fn same_family(a: &str, b: &str) -> bool {
    let norm = |f: &str| f.split(',').map(|p| p.trim().trim_matches(|c| c == '"' || c == '\'').to_ascii_lowercase()).collect::<Vec<_>>();
    norm(a) == norm(b)
}

fn same_style(a: &TextRun, b: &TextRun) -> bool {
    a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.strikethrough == b.strikethrough
        && a.color == b.color
        && a.font_size_px == b.font_size_px
        && a.font_family == b.font_family
}

fn has_style(r: &TextRun) -> bool {
    !same_style(r, &TextRun::default())
}

/// The spec's runs, or one run of the whole text carrying the object underline.
pub fn spec_runs(spec: &TextSpec) -> Vec<TextRun> {
    match &spec.runs {
        Some(r) if !r.is_empty() => r.clone(),
        _ => vec![TextRun { text: spec.text.clone(), underline: spec.underline.filter(|u| *u), ..Default::default() }],
    }
}

/// Split the run containing char `i` so that a run boundary lies at `i`.
pub fn split_at(runs: &mut Vec<TextRun>, i: usize) {
    let mut start = 0;
    for k in 0..runs.len() {
        let n = char_len(&runs[k].text);
        if i > start && i < start + n {
            let b = byte_at(&runs[k].text, i - start);
            let tail = runs[k].text.split_off(b);
            let r = TextRun { text: tail, ..runs[k].clone() };
            runs.insert(k + 1, r);
            return;
        }
        start += n;
    }
}

/// Join adjacent same-style runs and drop empty ones.
pub fn merge(runs: Vec<TextRun>) -> Vec<TextRun> {
    let mut out: Vec<TextRun> = Vec::with_capacity(runs.len());
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if same_style(last, &r) => last.text.push_str(&r.text),
            _ => out.push(r),
        }
    }
    out
}

/// Strip fields equal to the base, merge, and store into `spec` (text mirrors runs).
pub fn normalize(spec: &mut TextSpec, runs: Vec<TextRun>) {
    let base_color = normalize_color(&spec.color);
    let runs: Vec<TextRun> = runs
        .into_iter()
        .map(|mut r| {
            if r.bold == Some(spec.bold) {
                r.bold = None;
            }
            if r.italic == Some(spec.italic) {
                r.italic = None;
            }
            if r.underline == Some(false) {
                r.underline = None;
            }
            if r.strikethrough == Some(false) {
                r.strikethrough = None;
            }
            r.color = r.color.map(|c| normalize_color(&c)).filter(|c| *c != base_color);
            r.font_size_px = r.font_size_px.filter(|s| (s - spec.font_size_px).abs() > 1e-6);
            r.font_family = r.font_family.filter(|f| !same_family(f, &spec.font_family));
            r
        })
        .collect();
    let runs = merge(runs);
    spec.text = runs.iter().map(|r| r.text.as_str()).collect();
    // Underlined throughout and otherwise plain: that is the object underline.
    let all_underlined = !runs.is_empty() && runs.iter().all(|r| r.underline == Some(true));
    if runs.is_empty() {
        spec.runs = None; // no text: the object style stands
    } else if all_underlined && !runs.iter().any(|r| has_style(&TextRun { underline: None, ..r.clone() })) {
        spec.runs = None;
        spec.underline = Some(true);
    } else if runs.iter().any(has_style) {
        spec.runs = Some(runs);
        spec.underline = None; // runs decide (rich-text spec rule 2)
    } else {
        spec.runs = None;
        spec.underline = None;
    }
}

/// Apply a toolbar command to the whole object (nothing selected): the object
/// field changes and the matching run overrides are dropped. Toggles turn off
/// only when every char already has the attribute.
pub fn apply_to_object(spec: &TextSpec, edit: &StyleEdit) -> TextSpec {
    let all = style_at(spec, 0..usize::MAX);
    let mut out = spec.clone();
    // No text yet: underline is the object flag; strikethrough has none.
    if spec.text.is_empty() && matches!(edit, StyleEdit::Underline | StyleEdit::Strikethrough) {
        if *edit == StyleEdit::Underline {
            out.underline = (!all.underline).then_some(true);
        }
        return out;
    }
    let mut runs = spec_runs(spec);
    match edit {
        StyleEdit::Bold => {
            out.bold = !all.bold;
            runs.iter_mut().for_each(|r| r.bold = None);
        }
        StyleEdit::Italic => {
            out.italic = !all.italic;
            runs.iter_mut().for_each(|r| r.italic = None);
        }
        StyleEdit::Underline => runs.iter_mut().for_each(|r| r.underline = (!all.underline).then_some(true)),
        StyleEdit::Strikethrough => runs.iter_mut().for_each(|r| r.strikethrough = (!all.strikethrough).then_some(true)),
        StyleEdit::Color(c) => {
            out.color = normalize_color(c);
            runs.iter_mut().for_each(|r| r.color = None);
        }
        StyleEdit::Font(f) => {
            out.font_family = f.clone();
            runs.iter_mut().for_each(|r| r.font_family = None);
        }
        StyleEdit::ScaleSize(k) => {
            out.font_size_px = step_size(spec.font_size_px, *k);
            runs.iter_mut().for_each(|r| r.font_size_px = r.font_size_px.map(|s| step_size(s, *k)));
        }
    }
    normalize(&mut out, runs);
    out
}

/// One A−/A+ step: `size·k`, rounded to 0.01 px, at least 2 px (PC:377-420).
fn step_size(size: f64, k: f64) -> f64 {
    ((size * k * 100.0).round() / 100.0).max(2.0)
}

fn clamp_range(spec_runs: &[TextRun], r: Range<usize>) -> Range<usize> {
    let n: usize = spec_runs.iter().map(|r| char_len(&r.text)).sum();
    r.start.min(n)..r.end.min(n)
}

/// Apply a toolbar command to chars `range` of the spec.
pub fn apply_style(spec: &TextSpec, range: Range<usize>, edit: &StyleEdit) -> TextSpec {
    let mut runs = spec_runs(spec);
    let range = clamp_range(&runs, range);
    if range.start >= range.end {
        return spec.clone();
    }
    split_at(&mut runs, range.start);
    split_at(&mut runs, range.end);
    let mut inside = Vec::new();
    let mut pos = 0;
    for (k, r) in runs.iter().enumerate() {
        let n = char_len(&r.text);
        if pos >= range.start && pos + n <= range.end && n > 0 {
            inside.push(k);
        }
        pos += n;
    }
    let all = |f: &dyn Fn(&TextRun) -> bool| inside.iter().all(|&k| f(&runs[k]));
    let on = match edit {
        StyleEdit::Bold => !all(&|r| r.bold.unwrap_or(spec.bold)),
        StyleEdit::Italic => !all(&|r| r.italic.unwrap_or(spec.italic)),
        StyleEdit::Underline => !all(&|r| r.underline.unwrap_or(false)),
        StyleEdit::Strikethrough => !all(&|r| r.strikethrough.unwrap_or(false)),
        _ => true,
    };
    for &k in &inside {
        let r = &mut runs[k];
        match edit {
            StyleEdit::Bold => r.bold = Some(on),
            StyleEdit::Italic => r.italic = Some(on),
            StyleEdit::Underline => r.underline = on.then_some(true),
            StyleEdit::Strikethrough => r.strikethrough = on.then_some(true),
            StyleEdit::Color(c) => r.color = Some(normalize_color(c)),
            StyleEdit::Font(f) => r.font_family = Some(f.clone()),
            StyleEdit::ScaleSize(f) => {
                let size = r.font_size_px.unwrap_or(spec.font_size_px);
                r.font_size_px = Some(step_size(size, *f));
            }
        }
    }
    let mut out = spec.clone();
    normalize(&mut out, runs);
    out
}

/// Retarget the runs after the plain text changed to `new` (typing, paste,
/// delete). Inserted text takes the style of the char before it, or the char
/// after it at the start.
pub fn replace_text(spec: &TextSpec, new: &str) -> TextSpec {
    if spec.runs.as_ref().is_none_or(|r| r.is_empty()) {
        return TextSpec { text: new.to_string(), ..spec.clone() };
    }
    let mut runs = spec_runs(spec);
    let old: Vec<char> = runs.iter().flat_map(|r| r.text.chars()).collect();
    let nw: Vec<char> = new.chars().collect();
    let p = old.iter().zip(&nw).take_while(|(a, b)| a == b).count();
    let max_s = old.len().min(nw.len()) - p;
    let s = old.iter().rev().zip(nw.iter().rev()).take(max_s).take_while(|(a, b)| a == b).count();
    let (del_end, ins) = (old.len() - s, nw[p..nw.len() - s].iter().collect::<String>());
    // The style the insertion inherits.
    let src = if p > 0 {
        Some(p - 1)
    } else if !old.is_empty() {
        Some(0)
    } else {
        None
    };
    let style = src.and_then(|i| run_at(&runs, i)).map(|r| TextRun { text: String::new(), ..r.clone() }).unwrap_or_default();
    split_at(&mut runs, p);
    split_at(&mut runs, del_end);
    let mut out_runs = Vec::with_capacity(runs.len() + 1);
    let mut pos = 0;
    let mut inserted = false;
    for r in runs {
        let n = char_len(&r.text);
        if !inserted && pos >= p {
            out_runs.push(TextRun { text: ins.clone(), ..style.clone() });
            inserted = true;
        }
        if !(pos >= p && pos + n <= del_end) {
            out_runs.push(r);
        }
        pos += n;
    }
    if !inserted {
        out_runs.push(TextRun { text: ins, ..style });
    }
    let mut out = spec.clone();
    normalize(&mut out, out_runs);
    out
}

fn run_at(runs: &[TextRun], i: usize) -> Option<&TextRun> {
    let mut pos = 0;
    for r in runs {
        let n = char_len(&r.text);
        if i < pos + n {
            return Some(r);
        }
        pos += n;
    }
    None
}

/// The resolved style shared by every char in `range` (a collapsed range reads
/// the char before the caret).
pub fn style_at(spec: &TextSpec, range: Range<usize>) -> RangeStyle {
    let runs = spec_runs(spec);
    let mut range = clamp_range(&runs, range);
    if range.start >= range.end {
        range = range.start.saturating_sub(1)..range.start.max(1);
    }
    let mut picked = Vec::new();
    let mut pos = 0;
    for r in &runs {
        let n = char_len(&r.text);
        if n > 0 && pos < range.end && pos + n > range.start {
            picked.push(r);
        }
        pos += n;
    }
    if picked.is_empty() {
        picked.push(&runs[0]);
    }
    fn common<T: PartialEq>(v: impl Iterator<Item = T>) -> Option<T> {
        let mut it = v;
        let first = it.next()?;
        it.all(|x| x == first).then_some(first)
    }
    RangeStyle {
        bold: picked.iter().all(|r| r.bold.unwrap_or(spec.bold)),
        italic: picked.iter().all(|r| r.italic.unwrap_or(spec.italic)),
        underline: picked.iter().all(|r| r.underline.unwrap_or(false)),
        strikethrough: picked.iter().all(|r| r.strikethrough.unwrap_or(false)),
        color: common(picked.iter().map(|r| normalize_color(r.color.as_deref().unwrap_or(&spec.color)))),
        size: common(picked.iter().map(|r| r.font_size_px.unwrap_or(spec.font_size_px))),
        family: common(picked.iter().map(|r| r.font_family.clone().unwrap_or_else(|| spec.font_family.clone()))),
    }
}

/// Scale every run's own size override by `k` (TR:135, object scaling).
pub fn scale_run_sizes(runs: Option<Vec<TextRun>>, k: f64) -> Option<Vec<TextRun>> {
    runs.map(|v| v.into_iter().map(|r| TextRun { font_size_px: r.font_size_px.map(|s| s * k), ..r }).collect())
}
