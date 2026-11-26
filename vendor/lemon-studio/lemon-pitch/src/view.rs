// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Pitch — original implementation (see params.rs note).

//! Lemon Pitch editor painting and hit regions: Newtone's layout in the Lemon design — the pitch grid with the
//! detected pitch trace and the orange segment blocks (drag one up or down to retune that segment), the
//! waveform under it, and the Center / Variation / Transition knobs along the bottom.

use std::sync::Arc;

use os_engine::Sample;
use os_theme::design::lemon as brand;
use os_theme::Palette;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{self, inst, State};
use os_ui::{Canvas, Color, Font, Rect};

use crate::dsp::{Analysis, HOP};

/// Painted size in logical pixels, as the other Lemon instruments.
pub const W: u32 = 568;
pub const H: u32 = 442;
pub const CROP_X: u32 = 4;
pub const CROP_Y: u32 = 1;
pub const EDITOR_W: u32 = 560;
pub const EDITOR_H: u32 = 437;
pub const STRIP_ROWS: u32 = 47;
pub const HOSTED_CROP_Y: u32 = CROP_Y + STRIP_ROWS;
pub const HOSTED_EDITOR_H: u32 = EDITOR_H - STRIP_ROWS;

const LABEL: Font = inst::LABEL;
const CAP: Font = inst::CAPTION;
const TITLE: Font = Font::new(13).bold();

/// Newtone's segment orange.
pub const SEGMENT: u32 = 0xf28c28;
/// The detected pitch trace.
pub const TRACE: u32 = 0xffd23e;

/// Notes visible on the grid at once.
pub const VISIBLE_NOTES: f32 = 30.0;

/// Read access to parameter values by id.
pub trait Model {
    fn norm(&self, id: &str) -> f32;
    fn plain(&self, id: &str) -> f32;
    fn text(&self, id: &str) -> String;
}

impl Model for os_plugkit::vals::Vals {
    fn norm(&self, id: &str) -> f32 {
        os_plugkit::vals::Vals::norm(self, id)
    }
    fn plain(&self, id: &str) -> f32 {
        os_plugkit::vals::Vals::plain(self, id)
    }
    fn text(&self, id: &str) -> String {
        os_plugkit::vals::Vals::text(self, id)
    }
}

/// What a region of the editor does when clicked or dragged.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Vertical drag / wheel; menu on right click.
    Knob(String),
    /// Opens the sample file dialog.
    LoadSample,
    /// A pitch segment block: drag vertically to move its target note.
    Block(usize),
    /// The grid itself (wheel scrolls the note range).
    Grid,
}

impl Target {
    pub fn param(&self) -> Option<&str> {
        match self {
            Target::Knob(s) => Some(s),
            _ => None,
        }
    }
}

/// Everything a hit region carries.
#[derive(Clone, Debug)]
pub struct Hit {
    pub r: Rect,
    pub t: Target,
}

/// The data the editor paints: the loaded sample and its analysis, snapshotted from the shared state.
#[derive(Clone)]
pub struct DataView {
    pub sample: Option<Arc<Sample>>,
    pub analysis: Option<Arc<Analysis>>,
    pub path: String,
    pub version: u32,
}

impl Default for DataView {
    fn default() -> Self {
        DataView { sample: None, analysis: None, path: String::new(), version: u32::MAX }
    }
}

#[derive(Clone, PartialEq)]
pub struct ViewState {
    /// The highest note row shown.
    pub note_hi: f32,
    pub mouse: Option<(f32, f32)>,
    pub pressed: Option<Target>,
    pub data: DataView,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState { note_hi: 86.0, mouse: None, pressed: None, data: DataView::default() }
    }
}

impl PartialEq for DataView {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.path == other.path
    }
}

pub struct Ui<'a> {
    pub c: &'a mut Canvas,
    pub t: &'a Palette,
    pub m: &'a dyn Model,
    pub st: &'a ViewState,
    pub hits: Vec<Hit>,
}

impl Ui<'_> {
    fn hit(&mut self, r: Rect, t: Target) {
        self.hits.push(Hit { r, t });
    }
    fn state(&self, r: Rect, t: &Target) -> State {
        let pressed = self.st.pressed.as_ref() == Some(t);
        let hover = !pressed && self.st.pressed.is_none() && self.st.mouse.is_some_and(|(x, y)| r.contains(x, y));
        State { hover, pressed, ..State::NORMAL }
    }
}

fn rr(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

/// The pitch grid's rectangle.
pub fn grid_rect() -> Rect {
    rr(16.0, 108.0, 536.0, 216.0)
}

/// The waveform strip's rectangle.
pub fn wave_rect() -> Rect {
    rr(16.0, 332.0, 536.0, 44.0)
}

/// Height of one note row on the grid.
pub fn row_h() -> f32 {
    grid_rect().h / VISIBLE_NOTES
}

/// The y of a note's row centre.
pub fn y_of_note(note_hi: f32, note: f32) -> f32 {
    let g = grid_rect();
    g.y + (note_hi - note) * row_h() - row_h() / 2.0
}

/// The note at painted y (fractional).
pub fn note_at_y(note_hi: f32, y: f32) -> f32 {
    let g = grid_rect();
    note_hi - (y - g.y) / row_h()
}

/// The x of a source frame.
pub fn x_of_frame(frame: usize, total: usize) -> f32 {
    let g = grid_rect();
    if total == 0 {
        return g.x;
    }
    g.x + frame as f32 / total as f32 * g.w
}

/// Paints the editor. `hosted` = the host's strip row carries the page tab and the hint line.
pub fn paint(c: &mut Canvas, t: &Palette, m: &dyn Model, st: &ViewState, hint: &str, hosted: bool) -> Vec<Hit> {
    let mut ui = Ui { c, t, m, st, hits: Vec::new() };
    ui.c.begin_frame(None);
    ui.c.clear(plug::BG);
    if !hosted {
        ui.c.text(TITLE, 16.0, 34.0, brand::TEXT, "LEMON PITCH");
        if !hint.is_empty() {
            ui.c.text_clip(LABEL, 190.0, 36.0, 360.0, plug::DIM, hint);
        }
    }
    load_row(&mut ui);
    grid(&mut ui);
    wave(&mut ui);
    knob_row(&mut ui);
    ui.hits
}

/// The load button and the loaded file's name.
fn load_row(ui: &mut Ui) {
    let r = rr(16.0, 76.0, 110.0, 24.0);
    let target = Target::LoadSample;
    let st = ui.state(r, &target);
    ui.c.fill_rrect(r, 5.0, if st.pressed { Color::rgb(0x1f1f1f) } else if st.hover { Color::rgb(0x474747) } else { Color::rgb(0x3d3d3d) });
    let lh = ui.c.line_height(LABEL);
    ui.c.text_clip(LABEL, r.x + 8.0, r.cy() - lh / 2.0, r.w - 16.0, brand::TEXT, "LOAD SAMPLE");
    ui.hit(r, target);
    let name = if ui.st.data.path.is_empty() {
        "No sample loaded — load a vocal or melody line, then play it with MIDI notes (note 60 = as recorded).".to_string()
    } else {
        ui.st.data.path.rsplit(['/', '\\']).next().unwrap_or("").to_string()
    };
    ui.c.text_clip(LABEL, 138.0, 82.0, 414.0, plug::DIM, &name);
}

/// The pitch grid: note rows, the detected trace, the segment blocks.
fn grid(ui: &mut Ui) {
    let g = grid_rect();
    ui.c.fill_rrect(g, 6.0, plug::WELL);
    let hi = ui.st.note_hi;
    // Note rows: shade the black keys, line each C.
    let lo = hi - VISIBLE_NOTES;
    let mut n = lo.ceil() as i32;
    while (n as f32) < hi {
        let y = y_of_note(hi, n as f32);
        let black = matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
        if black {
            ui.c.fill_rect(rr(g.x + 1.0, y - row_h() / 2.0, g.w - 2.0, row_h()), Color::rgb(0x262626));
        }
        if n.rem_euclid(12) == 0 {
            ui.c.fill_rect(rr(g.x + 1.0, y + row_h() / 2.0, g.w - 2.0, 1.0), plug::GRID);
            ui.c.text_clip(CAP, g.x + 4.0, y - row_h() / 2.0, 30.0, plug::DIM, &os_dsp::pitch::note_name(n));
        }
        n += 1;
    }
    ui.hit(g, Target::Grid);
    let (Some(sample), Some(a)) = (&ui.st.data.sample, &ui.st.data.analysis) else {
        ui.c.text_clip(LABEL, g.x + 12.0, g.cy() - 6.0, g.w - 24.0, plug::DIM, "The detected pitch and its segments will appear here.");
        return;
    };
    let total = sample.frames();
    // Segment blocks at their target notes.
    for (i, seg) in a.segments.iter().enumerate() {
        let x0 = x_of_frame(seg.start * HOP, total);
        let x1 = x_of_frame(seg.end * HOP, total);
        let y = y_of_note(hi, seg.target);
        let r = rr(x0 + 1.0, y - row_h() / 2.0 + 1.0, (x1 - x0 - 2.0).max(3.0), row_h() - 2.0);
        let target = Target::Block(i);
        let st = ui.state(r, &target);
        let col = Color::rgb(SEGMENT);
        ui.c.fill_rrect(r, 3.0, if st.pressed || st.hover { col } else { col.with_alpha(190) });
        ui.hit(r, target);
    }
    // The detected pitch trace.
    let col = Color::rgb(TRACE);
    for (i, p) in a.track.iter().enumerate() {
        let Some(p) = p else { continue };
        let x = x_of_frame(i * HOP, total);
        let y = y_of_note(hi, *p);
        if y >= g.y && y <= g.y + g.h {
            ui.c.fill_rect(rr(x, y - 1.0, (g.w / a.track.len().max(1) as f32).max(1.5), 2.0), col);
        }
    }
}

/// The waveform strip.
fn wave(ui: &mut Ui) {
    let r = wave_rect();
    ui.c.fill_rrect(r, 6.0, plug::WELL);
    let Some(sample) = &ui.st.data.sample else { return };
    let total = sample.frames().max(1);
    let cols = r.w as usize;
    let col = brand::MAIN.with_alpha(160);
    for cx in 0..cols {
        let a = cx * total / cols;
        let b = ((cx + 1) * total / cols).max(a + 1).min(total);
        let peak = sample.left[a..b].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let h = (peak * (r.h / 2.0 - 3.0)).max(0.5);
        ui.c.fill_rect(rr(r.x + cx as f32, r.cy() - h, 1.0, 2.0 * h), col);
    }
}

/// A lemon knob with its caption.
fn knob(ui: &mut Ui, id: &str, cx: f32, cy: f32, cap: &str) {
    let target = Target::Knob(id.into());
    let hr = rr(cx - 22.0, cy - 22.0, 44.0, 44.0);
    let st = ui.state(hr, &target);
    lemon::knob(ui.c, ui.t, cx, cy, 15.0, ui.m.norm(id), false, kit::KnobLook::Ball, st);
    ui.c.text_center(CAP, rr(cx - 44.0, cy + 20.0, 88.0, 12.0), plug::DIM, cap);
    ui.hit(hr, target);
}

/// The global correction knobs.
fn knob_row(ui: &mut Ui) {
    let y = 404.0;
    knob(ui, "center", 70.0, y, "CENTER");
    knob(ui, "variation", 190.0, y, "VARIATION");
    knob(ui, "transition", 310.0, y, "TRANSITION");
    knob(ui, "gain", 430.0, y, "GAIN");
    ui.c.text_clip(CAP, 478.0, y - 16.0, 80.0, plug::DIM, "Drag a block");
    ui.c.text_clip(CAP, 478.0, y - 4.0, 80.0, plug::DIM, "to retune it.");
}

/// The hint line for a hovered target.
pub fn hint(t: &Target, m: &dyn Model, st: &ViewState) -> String {
    match t {
        Target::Knob(id) => format!("{}: {}", pretty(id), m.text(id)),
        Target::LoadSample => "Load a sample to correct".to_string(),
        Target::Grid => "Pitch grid — wheel scrolls the notes, drag a block to retune it".to_string(),
        Target::Block(i) => match st.data.analysis.as_ref().and_then(|a| a.segments.get(*i)) {
            Some(seg) => format!(
                "Segment {}: detected {} → target {} (drag to retune)",
                i + 1,
                os_dsp::pitch::note_name(seg.median.round() as i32),
                os_dsp::pitch::note_name(seg.target.round() as i32)
            ),
            None => String::new(),
        },
    }
}

/// A readable name for a parameter id.
pub fn pretty(id: &str) -> String {
    id.replace('_', " ")
}
