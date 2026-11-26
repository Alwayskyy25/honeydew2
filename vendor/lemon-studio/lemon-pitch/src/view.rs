// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Pitch — original implementation (see params.rs note).

//! Lemon Pitch editor painting and hit regions: Newtone's layout in the Lemon design — a piano ruler on the
//! left, the sample's waveform as the grid's background, the detected pitch as a red trace, and the note
//! blocks (a ghost at the target semitone, the block itself spanning the segment's detected pitch range
//! with the trace running through it; drag a block up or down to retune that segment). The grid zooms both
//! ways, like Newtone's: horizontally over time, vertically over the pitch range, with scrolling and pan.
//! Under the grid: snap-to-scale controls and the Center / Variation / Transition knobs.

use std::sync::Arc;

use os_engine::Sample;
use os_theme::Palette;
use os_theme::design::lemon as brand;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{self, State, inst};
use os_ui::{Canvas, Color, Font, Rect};

use crate::dsp::{Analysis, HOP};
use crate::params::{NOTE_NAMES, SCALES};

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

/// Newtone's note-block teal.
pub const SEGMENT: u32 = 0x35b5ac;
/// The detected pitch trace (Newtone draws it red over the blocks).
pub const TRACE: u32 = 0xe5484d;

/// Width of the piano ruler at the grid's left.
pub const KEYS_W: f32 = 40.0;
/// Vertical zoom limits (notes on screen).
pub const MIN_VISIBLE: f32 = 6.0;
pub const MAX_VISIBLE: f32 = 60.0;
/// Horizontal zoom limit (times full view).
pub const MAX_ZOOM_X: f32 = 32.0;

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
    /// A 0 / 1 parameter toggled on click.
    Toggle(String),
    /// Sets a parameter to a plain value on click.
    Set(String, f32),
    /// Opens the sample file dialog.
    LoadSample,
    /// Exports the corrected audio to the sample library.
    Export,
    /// A pitch segment block: drag vertically to move its target note.
    Block(usize),
    /// The grid itself (wheel scrolls / zooms, drag pans).
    Grid,
    /// Horizontal zoom by a factor (time).
    ZoomH(f32),
    /// Vertical zoom by a factor (pitch range).
    ZoomV(f32),
    /// Reset both zooms to the full view.
    ZoomFull,
}

impl Target {
    pub fn param(&self) -> Option<&str> {
        match self {
            Target::Knob(s) | Target::Toggle(s) | Target::Set(s, _) => Some(s),
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
        DataView {
            sample: None,
            analysis: None,
            path: String::new(),
            version: u32::MAX,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct ViewState {
    /// The highest note row shown.
    pub note_hi: f32,
    /// Vertical zoom: how many note rows are on screen.
    pub visible: f32,
    /// Horizontal zoom: 1 = the whole sample, up to MAX_ZOOM_X.
    pub zoom_x: f32,
    /// Horizontal scroll: 0..1 across the hidden range (0 when zoom_x == 1).
    pub scroll: f32,
    pub mouse: Option<(f32, f32)>,
    pub pressed: Option<Target>,
    pub data: DataView,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            note_hi: 86.0,
            visible: 30.0,
            zoom_x: 1.0,
            scroll: 0.0,
            mouse: None,
            pressed: None,
            data: DataView::default(),
        }
    }
}

impl PartialEq for DataView {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.path == other.path
    }
}

impl ViewState {
    /// The first visible source frame and the visible frame count, for a sample of `total` frames.
    pub fn window(&self, total: usize) -> (f32, f32) {
        let total = total.max(1) as f32;
        let win = total / self.zoom_x.max(1.0);
        let start = self.scroll.clamp(0.0, 1.0) * (total - win).max(0.0);
        (start, win)
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
        let hover = !pressed
            && self.st.pressed.is_none()
            && self.st.mouse.is_some_and(|(x, y)| r.contains(x, y));
        State {
            hover,
            pressed,
            ..State::NORMAL
        }
    }
}

fn rr(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

/// The whole grid well, piano ruler included.
pub fn grid_outer() -> Rect {
    rr(16.0, 108.0, 536.0, 236.0)
}

/// The piano ruler's rectangle.
pub fn keys_rect() -> Rect {
    let o = grid_outer();
    rr(o.x, o.y, KEYS_W, o.h)
}

/// The pitch grid's rectangle (right of the piano ruler).
pub fn grid_rect() -> Rect {
    let o = grid_outer();
    rr(o.x + KEYS_W, o.y, o.w - KEYS_W, o.h)
}

/// Height of one note row on the grid.
pub fn row_h(visible: f32) -> f32 {
    grid_rect().h / visible.max(1.0)
}

/// The y of a note's row centre.
pub fn y_of_note(note_hi: f32, visible: f32, note: f32) -> f32 {
    let g = grid_rect();
    g.y + (note_hi - note) * row_h(visible) - row_h(visible) / 2.0
}

/// The note at painted y (fractional).
pub fn note_at_y(note_hi: f32, visible: f32, y: f32) -> f32 {
    let g = grid_rect();
    note_hi - (y - g.y) / row_h(visible)
}

/// The x of a source frame under the current zoom/scroll window.
pub fn x_of_frame(st: &ViewState, frame: usize, total: usize) -> f32 {
    let g = grid_rect();
    let (start, win) = st.window(total);
    g.x + (frame as f32 - start) / win * g.w
}

/// The source frame at painted x under the current zoom/scroll window.
pub fn frame_at_x(st: &ViewState, x: f32, total: usize) -> f32 {
    let g = grid_rect();
    let (start, win) = st.window(total);
    start + (x - g.x) / g.w * win
}

/// Paints the editor. `hosted` = the host's strip row carries the page tab and the hint line.
pub fn paint(
    c: &mut Canvas,
    t: &Palette,
    m: &dyn Model,
    st: &ViewState,
    hint: &str,
    hosted: bool,
) -> Vec<Hit> {
    let mut ui = Ui {
        c,
        t,
        m,
        st,
        hits: Vec::new(),
    };
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
    snap_row(&mut ui);
    knob_row(&mut ui);
    ui.hits
}

/// A small labelled button.
fn button(ui: &mut Ui, r: Rect, label: &str, active: bool, target: Target) {
    let st = ui.state(r, &target);
    let bg = if active {
        brand::MAIN.with_alpha(90)
    } else if st.pressed {
        Color::rgb(0x1f1f1f)
    } else if st.hover {
        Color::rgb(0x474747)
    } else {
        Color::rgb(0x3d3d3d)
    };
    ui.c.fill_rrect(r, 5.0, bg);
    let lh = ui.c.line_height(LABEL);
    ui.c.text_clip(
        LABEL,
        r.x + 6.0,
        r.cy() - lh / 2.0,
        r.w - 12.0,
        if active { brand::TEXT } else { plug::DIM },
        label,
    );
    ui.hit(r, target);
}

/// The load button, the loaded file's name, and the zoom buttons.
fn load_row(ui: &mut Ui) {
    let r = rr(16.0, 76.0, 110.0, 24.0);
    let target = Target::LoadSample;
    let st = ui.state(r, &target);
    ui.c.fill_rrect(
        r,
        5.0,
        if st.pressed {
            Color::rgb(0x1f1f1f)
        } else if st.hover {
            Color::rgb(0x474747)
        } else {
            Color::rgb(0x3d3d3d)
        },
    );
    let lh = ui.c.line_height(LABEL);
    ui.c.text_clip(
        LABEL,
        r.x + 8.0,
        r.cy() - lh / 2.0,
        r.w - 16.0,
        brand::TEXT,
        "LOAD SAMPLE",
    );
    ui.hit(r, target);
    let name = if ui.st.data.path.is_empty() {
        "No sample loaded".to_string()
    } else {
        ui.st
            .data
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .to_string()
    };
    ui.c.text_clip(LABEL, 138.0, 82.0, 200.0, plug::DIM, &name);
    // Zoom controls (Newtone: Ctrl+wheel zooms time, Alt+wheel zooms pitch; these are the touch route).
    button(
        ui,
        rr(348.0, 76.0, 34.0, 24.0),
        "H-",
        false,
        Target::ZoomH(0.5),
    );
    button(
        ui,
        rr(386.0, 76.0, 34.0, 24.0),
        "H+",
        false,
        Target::ZoomH(2.0),
    );
    button(
        ui,
        rr(424.0, 76.0, 34.0, 24.0),
        "V-",
        false,
        Target::ZoomV(0.5),
    );
    button(
        ui,
        rr(462.0, 76.0, 34.0, 24.0),
        "V+",
        false,
        Target::ZoomV(2.0),
    );
    button(
        ui,
        rr(500.0, 76.0, 52.0, 24.0),
        "FULL",
        false,
        Target::ZoomFull,
    );
}

/// The pitch grid: piano ruler, waveform background, note rows, segment blocks, the pitch trace.
fn grid(ui: &mut Ui) {
    let o = grid_outer();
    let g = grid_rect();
    ui.c.fill_rrect(o, 6.0, plug::WELL);
    let hi = ui.st.note_hi;
    let vis = ui.st.visible;
    let total = ui.st.data.sample.as_ref().map_or(0, |s| s.frames());
    // Waveform background (Newtone: note volume shows as the background waveform), visible window only.
    if let Some(sample) = &ui.st.data.sample {
        let (start, win) = ui.st.window(total);
        let cols = g.w as usize;
        let col = brand::MAIN.with_alpha(70);
        for cx in 0..cols {
            let a = (start + cx as f32 / cols.max(1) as f32 * win) as usize;
            let b = (start + (cx + 1) as f32 / cols.max(1) as f32 * win) as usize;
            let (a, b) = (a.min(total), b.max(a + 1).min(total.max(1)));
            if a >= total || b <= a {
                continue;
            }
            let peak = sample.left[a..b].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let h = (peak * (g.h / 2.0 - 4.0)).max(0.5);
            ui.c.fill_rect(rr(g.x + cx as f32, g.cy() - h, 1.0, 2.0 * h), col);
        }
    }
    // Note rows: shade the black keys, line each C.
    let lo = hi - vis;
    let mut n = lo.ceil() as i32;
    while (n as f32) < hi {
        let y = y_of_note(hi, vis, n as f32);
        let black = matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
        if black {
            ui.c.fill_rect(
                rr(g.x + 1.0, y - row_h(vis) / 2.0, g.w - 2.0, row_h(vis)),
                Color::rgb(0x262626),
            );
        }
        if n.rem_euclid(12) == 0 {
            ui.c.fill_rect(
                rr(g.x + 1.0, y + row_h(vis) / 2.0, g.w - 2.0, 1.0),
                plug::GRID,
            );
        }
        n += 1;
    }
    keys(ui);
    ui.hit(g, Target::Grid);
    let (Some(_sample), Some(a)) = (&ui.st.data.sample, &ui.st.data.analysis) else {
        ui.c.text_clip(
            LABEL,
            g.x + 12.0,
            g.cy() - 6.0,
            g.w - 24.0,
            plug::DIM,
            "The detected pitch and its segments will appear here.",
        );
        return;
    };
    // Segment blocks, Newtone style: a ghost at the target semitone, the block spanning the segment's
    // detected pitch range, and a brighter line at the segment's average (median) pitch.
    for (i, seg) in a.segments.iter().enumerate() {
        let x0 = x_of_frame(ui.st, seg.start * HOP, total);
        let x1 = x_of_frame(ui.st, seg.end * HOP, total);
        if x1 < g.x || x0 > g.x + g.w {
            continue;
        }
        let (x0, x1) = (x0.max(g.x), x1.min(g.x + g.w));
        let target = Target::Block(i);
        let st = ui.state(rr(x0, g.y, (x1 - x0).max(1.0), g.h), &target);
        let col = Color::rgb(SEGMENT);
        // Ghost at the target note.
        let gy = y_of_note(hi, vis, seg.target);
        ui.c.fill_rrect(
            rr(
                x0 + 1.0,
                gy - row_h(vis) / 2.0,
                (x1 - x0 - 2.0).max(3.0),
                row_h(vis),
            ),
            2.0,
            col.with_alpha(55),
        );
        // The block over the detected range.
        let y_top = y_of_note(hi, vis, seg.hi) - row_h(vis) / 2.0;
        let y_bot = y_of_note(hi, vis, seg.lo) + row_h(vis) / 2.0;
        let r = rr(
            x0 + 1.0,
            y_top,
            (x1 - x0 - 2.0).max(3.0),
            (y_bot - y_top).max(row_h(vis) * 0.8),
        );
        ui.c.fill_rrect(
            r,
            3.0,
            if st.pressed || st.hover {
                col.with_alpha(215)
            } else {
                col.with_alpha(165)
            },
        );
        // The average-pitch line.
        let my = y_of_note(hi, vis, seg.median);
        ui.c.fill_rect(rr(r.x, my - 1.0, r.w, 2.0), col);
        ui.hit(r, target);
    }
    // The detected pitch trace, drawn over the blocks.
    let col = Color::rgb(TRACE);
    let (start, win) = ui.st.window(total);
    for (i, p) in a.track.iter().enumerate() {
        let Some(p) = p else { continue };
        let f = i * HOP;
        if (f as f32) < start || (f as f32) > start + win {
            continue;
        }
        let x = x_of_frame(ui.st, f, total);
        let y = y_of_note(hi, vis, *p);
        if y >= g.y && y <= g.y + g.h {
            ui.c.fill_rect(
                rr(
                    x,
                    y - 1.0,
                    (g.w / a.track.len().max(1) as f32 * ui.st.zoom_x).max(1.5),
                    2.0,
                ),
                col,
            );
        }
    }
}

/// The piano ruler: white keys with black keys over them, note names on the C keys.
fn keys(ui: &mut Ui) {
    let kr = keys_rect();
    let hi = ui.st.note_hi;
    let vis = ui.st.visible;
    ui.c.fill_rect(rr(kr.x, kr.y, kr.w, kr.h), Color::rgb(0xd8d8d8));
    let lo = hi - vis;
    let mut n = lo.ceil() as i32;
    while (n as f32) < hi {
        let y = y_of_note(hi, vis, n as f32);
        let rh = row_h(vis);
        if !matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10) {
            ui.c.fill_rect(rr(kr.x, y + rh / 2.0, kr.w, 1.0), Color::rgb(0x9a9a9a));
        }
        if n.rem_euclid(12) == 0 {
            ui.c.text_clip(
                CAP,
                kr.x + 3.0,
                y - rh / 2.0,
                kr.w - 4.0,
                Color::rgb(0x333333),
                &os_dsp::pitch::note_name(n),
            );
        }
        n += 1;
    }
    // Black keys on top.
    let mut n = lo.ceil() as i32;
    while (n as f32) < hi {
        let y = y_of_note(hi, vis, n as f32);
        let rh = row_h(vis);
        if matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10) {
            ui.c.fill_rect(
                rr(kr.x, y - rh / 2.0, kr.w * 0.62, rh),
                Color::rgb(0x1c1c1c),
            );
        }
        n += 1;
    }
}

/// Snap-to-scale controls (Newtone's Edit-menu pair, put on a row of their own).
fn snap_row(ui: &mut Ui) {
    let y = 356.0;
    let snap = ui.m.plain("snap") > 0.5;
    button(
        ui,
        rr(16.0, y, 76.0, 22.0),
        "SNAP",
        snap,
        Target::Toggle("snap".into()),
    );
    ui.c.text(CAP, 100.0, y + 4.0, plug::DIM, "SCALE");
    let scale = ui.m.plain("scale").round() as i32;
    // Scale stepper.
    let sid = "scale";
    button(
        ui,
        rr(140.0, y, 20.0, 22.0),
        "-",
        false,
        Target::Set(sid.into(), (scale - 1).max(0) as f32),
    );
    ui.c.text_center(
        LABEL,
        rr(162.0, y + 3.0, 76.0, 16.0),
        brand::TEXT,
        SCALES[scale.clamp(0, 2) as usize],
    );
    button(
        ui,
        rr(240.0, y, 20.0, 22.0),
        "+",
        false,
        Target::Set(sid.into(), (scale + 1).min(2) as f32),
    );
    ui.c.text(CAP, 276.0, y + 4.0, plug::DIM, "ROOT");
    let root = ui.m.plain("root").round() as i32;
    button(
        ui,
        rr(310.0, y, 20.0, 22.0),
        "-",
        false,
        Target::Set("root".into(), (root - 1).max(0) as f32),
    );
    ui.c.text_center(
        LABEL,
        rr(332.0, y + 3.0, 40.0, 16.0),
        brand::TEXT,
        NOTE_NAMES[root.clamp(0, 11) as usize],
    );
    button(
        ui,
        rr(374.0, y, 20.0, 22.0),
        "+",
        false,
        Target::Set("root".into(), (root + 1).min(11) as f32),
    );
    ui.c.text_clip(
        CAP,
        408.0,
        y + 4.0,
        144.0,
        plug::DIM,
        "Wheel: scroll · Ctrl: zoom time",
    );
    ui.c.text_clip(
        CAP,
        408.0,
        y + 14.0,
        144.0,
        plug::DIM,
        "Alt: zoom pitch · drag: pan",
    );
}

/// A lemon knob with its caption.
fn knob(ui: &mut Ui, id: &str, cx: f32, cy: f32, cap: &str) {
    let target = Target::Knob(id.into());
    let hr = rr(cx - 22.0, cy - 22.0, 44.0, 44.0);
    let st = ui.state(hr, &target);
    lemon::knob(
        ui.c,
        ui.t,
        cx,
        cy,
        15.0,
        ui.m.norm(id),
        false,
        kit::KnobLook::Ball,
        st,
    );
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
    button(
        ui,
        rr(474.0, y - 14.0, 78.0, 28.0),
        "EXPORT",
        false,
        Target::Export,
    );
}

/// The hint line for a hovered target.
pub fn hint(t: &Target, m: &dyn Model, st: &ViewState) -> String {
    match t {
        Target::Knob(id) | Target::Toggle(id) => format!("{}: {}", pretty(id), m.text(id)),
        Target::Set(id, _) => pretty(id).to_string(),
        Target::LoadSample => "Load a sample to correct".to_string(),
        Target::Export => {
            "Export the corrected audio to the sample library (loadable by any plugin)".to_string()
        }
        Target::Grid => {
            "Pitch grid — wheel scrolls, Ctrl+wheel zooms time, Alt+wheel zooms pitch, drag pans"
                .to_string()
        }
        Target::ZoomH(f) => {
            if *f > 1.0 {
                "Zoom in horizontally (time)".to_string()
            } else {
                "Zoom out horizontally (time)".to_string()
            }
        }
        Target::ZoomV(f) => {
            if *f > 1.0 {
                "Zoom in vertically (pitch)".to_string()
            } else {
                "Zoom out vertically (pitch)".to_string()
            }
        }
        Target::ZoomFull => "Zoom full — show the whole sample".to_string(),
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
