// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — original implementation (see params.rs note).

//! Lemon Additive editor painting and hit regions (platform independent), in the Lemon design: the two parts
//! across the top, the drawable harmonic spectrum in the middle, the part's knobs under it and the global
//! envelope / mix knobs along the bottom.

use os_theme::design::lemon as brand;
use os_theme::Palette;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{self, inst, State};
use os_ui::{Canvas, Color, Font, Rect};

use crate::params::{BANDS, PART_MODES, SHAPES};

/// Painted size in logical pixels (a window minus its title bar), as the other Lemon instruments.
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

/// Part accent colours: A green, B red — the piano roll colours that select them per note.
pub const PART_COLORS: [u32; 2] = [0x51c878, 0xe5484d];

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
    /// Chooses a value from the list menu.
    List(String),
    /// The editor shows part A (0) or part B (1).
    PartTab(u8),
    /// A spectrum band of the shown part (drag vertically).
    Band(usize),
}

impl Target {
    pub fn param(&self) -> Option<&str> {
        match self {
            Target::Knob(s) | Target::Toggle(s) | Target::Set(s, _) | Target::List(s) => Some(s),
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

#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    /// The part the editor shows (0 = A, 1 = B).
    pub part: u8,
    pub mouse: Option<(f32, f32)>,
    pub pressed: Option<Target>,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState { part: 0, mouse: None, pressed: None }
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

/// The shown part's parameter id prefix.
pub fn prefix(part: u8) -> &'static str {
    if part == 0 { "aa" } else { "ab" }
}

/// Band parameter id of the shown part.
pub fn band_id(part: u8, i: usize) -> String {
    format!("{}_{}", prefix(part), format!("band{}", i + 1))
}

/// The spectrum well's rectangle.
pub fn spectrum_rect() -> Rect {
    Rect::new(16.0, 150.0, 536.0, 150.0)
}

/// Paints the editor. `hosted` = the host's strip row carries the page tab and the hint line.
pub fn paint(c: &mut Canvas, t: &Palette, m: &dyn Model, st: &ViewState, hint: &str, hosted: bool) -> Vec<Hit> {
    let mut ui = Ui { c, t, m, st, hits: Vec::new() };
    ui.c.begin_frame(None);
    ui.c.clear(plug::BG);
    if !hosted {
        ui.c.text(TITLE, 16.0, 34.0, brand::TEXT, "LEMON ADDITIVE");
        if !hint.is_empty() {
            ui.c.text_clip(LABEL, 190.0, 36.0, 360.0, plug::DIM, hint);
        }
    }
    part_row(&mut ui);
    shape_row(&mut ui);
    spectrum(&mut ui);
    part_knobs(&mut ui);
    global_row(&mut ui);
    ui.hits
}

fn rr(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

/// A small labelled button; returns its rect.
fn button(ui: &mut Ui, r: Rect, label: &str, active: bool, target: Target, col: Color) {
    let st = ui.state(r, &target);
    let bg = if active {
        col.with_alpha(90)
    } else if st.pressed {
        Color::rgb(0x1f1f1f)
    } else if st.hover {
        Color::rgb(0x474747)
    } else {
        Color::rgb(0x3d3d3d)
    };
    ui.c.fill_rrect(r, 5.0, bg);
    let lh = ui.c.line_height(LABEL);
    ui.c.text_clip(LABEL, r.x + 6.0, r.cy() - lh / 2.0, r.w - 12.0, if active { brand::TEXT } else { plug::DIM }, label);
    ui.hit(r, target);
}

/// The part selector row: PART A / PART B tabs, the shown part's on switch.
fn part_row(ui: &mut Ui) {
    let y = 78.0;
    for p in 0..2u8 {
        let r = rr(16.0 + p as f32 * 110.0, y, 102.0, 26.0);
        let col = Color::rgb(PART_COLORS[p as usize]);
        button(ui, r, if p == 0 { "PART A" } else { "PART B" }, ui.st.part == p, Target::PartTab(p), col);
    }
    let pre = prefix(ui.st.part);
    let on_id = format!("{pre}_on");
    let on = ui.m.plain(&on_id) > 0.5;
    button(ui, rr(244.0, y, 70.0, 26.0), if on { "ON" } else { "OFF" }, on, Target::Toggle(on_id), Color::rgb(PART_COLORS[ui.st.part as usize]));
    // Part mode (global): how notes choose their part.
    let mode = ui.m.plain("partmode").round() as usize;
    ui.c.text(CAP, 330.0, y + 2.0, plug::DIM, "PART MODE");
    let labels = ["MIX", "A", "B", "NOTE CH"];
    for (i, label) in labels.iter().enumerate() {
        let r = rr(330.0 + i as f32 * 56.0, y + 14.0, 52.0, 18.0);
        button(ui, r, label, mode == i, Target::Set("partmode".into(), i as f32), brand::MAIN);
    }
    let _ = PART_MODES;
}

/// The shown part's shape choices.
fn shape_row(ui: &mut Ui) {
    let pre = prefix(ui.st.part);
    let id = format!("{pre}_shape");
    let cur = ui.m.plain(&id).round() as usize;
    ui.c.text(CAP, 16.0, 116.0, plug::DIM, "TIMBRE");
    for (i, name) in SHAPES.iter().enumerate() {
        let r = rr(70.0 + i as f32 * 80.0, 112.0, 74.0, 22.0);
        button(ui, r, name, cur == i, Target::Set(id.clone(), i as f32), brand::MAIN);
    }
}

/// The shape recipe, mirrored from the DSP for display.
fn shape_amp(shape: i32, k: usize) -> f32 {
    let kf = k as f32;
    match shape {
        0 => 1.0 / kf,
        1 => {
            if k % 2 == 1 { 1.0 / kf } else { 0.0 }
        }
        2 => {
            if k % 2 == 1 { 1.0 / (kf * kf) } else { 0.0 }
        }
        3 => {
            if k == 1 { 1.0 } else { 0.0 }
        }
        4 => match k {
            1 | 2 | 3 | 4 | 6 | 8 => 1.0 / kf.sqrt(),
            _ => 0.0,
        },
        _ => 1.0 / kf.sqrt(),
    }
}

/// The drawable spectrum: the partial curve (shape x bands x brightness) behind the 12 band bars.
fn spectrum(ui: &mut Ui) {
    let area = spectrum_rect();
    ui.c.fill_rrect(area, 6.0, plug::WELL);
    ui.c.fill_rect(rr(area.x, area.y + area.h - 1.0, area.w, 1.0), plug::WELL_LINE);
    let pre = prefix(ui.st.part);
    let shape = ui.m.plain(&format!("{pre}_shape")).round() as i32;
    let bright = ui.m.norm("bright");
    let tilt = (0.5 - bright) * -1.4;
    let bands: Vec<f32> = (0..BANDS).map(|i| ui.m.norm(&band_id(ui.st.part, i))).collect();
    let band_gain = |k: usize| -> f32 {
        if k <= 1 {
            return bands[0];
        }
        let x = (k as f32).ln() / 64f32.ln() * (BANDS - 1) as f32;
        let i = (x as usize).min(BANDS - 2);
        let f = x - i as f32;
        bands[i] * (1.0 - f) + bands[i + 1] * f
    };
    // The partial curve as vertical stems.
    let col = Color::rgb(PART_COLORS[ui.st.part as usize]);
    for k in 1..=64usize {
        let a = (shape_amp(shape, k) * band_gain(k) * (k as f32).powf(tilt)).clamp(0.0, 1.0);
        if a <= 0.01 {
            continue;
        }
        let x = area.x + 8.0 + (k as f32 - 1.0) / 63.0 * (area.w - 16.0);
        let h = a * (area.h - 30.0);
        ui.c.fill_rect(rr(x, area.y + area.h - 8.0 - h, 2.0, h), col.with_alpha(150));
    }
    // The band bars.
    let bw = area.w / BANDS as f32;
    for i in 0..BANDS {
        let v = bands[i];
        let r = rr(area.x + i as f32 * bw + 6.0, area.y + 8.0, bw - 12.0, area.h - 16.0);
        let bh = v * (area.h - 40.0);
        ui.c.fill_rrect(rr(r.x, r.y + r.h - bh, r.w, bh.max(2.0)), 3.0, col.with_alpha(70));
        ui.c.fill_rect(rr(r.x, r.y + r.h - bh - 2.0, r.w, 3.0), col);
        ui.hit(r, Target::Band(i));
    }
    ui.c.text(CAP, area.x + 2.0, area.y - 14.0, plug::DIM, "SPECTRUM — drag the bands; the stems show the partials");
}

/// A lemon knob with its caption and value.
fn knob(ui: &mut Ui, id: &str, cx: f32, cy: f32, cap: &str, bipolar: bool) {
    let target = Target::Knob(id.into());
    let hr = rr(cx - 22.0, cy - 22.0, 44.0, 44.0);
    let st = ui.state(hr, &target);
    lemon::knob(ui.c, ui.t, cx, cy, 15.0, ui.m.norm(id), bipolar, kit::KnobLook::Ball, st);
    ui.c.text_center(CAP, rr(cx - 40.0, cy + 20.0, 80.0, 12.0), plug::DIM, cap);
    ui.hit(hr, target);
}

/// The shown part's knobs: level, detune, octave steppers.
fn part_knobs(ui: &mut Ui) {
    let pre = prefix(ui.st.part);
    let y = 336.0;
    knob(ui, &format!("{pre}_level"), 60.0, y, "LEVEL", false);
    knob(ui, &format!("{pre}_detune"), 140.0, y, "DETUNE", true);
    // Octave stepper.
    let oct_id = format!("{pre}_octave");
    let oct = ui.m.plain(&oct_id).round() as i32;
    ui.c.text(CAP, 196.0, y - 22.0, plug::DIM, "OCTAVE");
    button(ui, rr(190.0, y - 12.0, 30.0, 24.0), "-", false, Target::Set(oct_id.clone(), (oct - 1).max(-2) as f32), brand::MAIN);
    ui.c.text_center(LABEL, rr(224.0, y - 14.0, 40.0, 16.0), brand::TEXT, &format!("{oct:+}"));
    button(ui, rr(268.0, y - 12.0, 30.0, 24.0), "+", false, Target::Set(oct_id.clone(), (oct + 1).min(2) as f32), brand::MAIN);
    ui.c.text(CAP, 330.0, y - 22.0, plug::DIM, "Drag a knob, right-click for its menu (reset / type a value / automation).");
    ui.c.text(CAP, 330.0, y - 8.0, plug::DIM, "NOTE CH mode: piano-roll channel 1 = Part A, channel 2 = Part B.");
}

/// The global row: part mix, brightness, prism, the ADSR and unison / gain.
fn global_row(ui: &mut Ui) {
    let y = 402.0;
    knob(ui, "mix", 40.0, y, "MIX A/B", false);
    knob(ui, "bright", 110.0, y, "BRIGHT", false);
    knob(ui, "prism", 180.0, y, "PRISM", false);
    knob(ui, "attack", 260.0, y, "ATTACK", false);
    knob(ui, "decay", 330.0, y, "DECAY", false);
    knob(ui, "sustain", 400.0, y, "SUSTAIN", false);
    knob(ui, "release", 470.0, y, "RELEASE", false);
    // Unison count stepper + gain knob at the right edge would crowd; unison is a Set stepper above the row.
    let u = ui.m.plain("unison").round() as i32;
    ui.c.text(CAP, 516.0, y - 34.0, plug::DIM, "UNI");
    button(ui, rr(508.0, y - 24.0, 44.0, 20.0), &format!("{u} vox"), false, Target::Set("unison".into(), if u >= 4 { 1.0 } else { (u + 1) as f32 }), brand::MAIN);
    knob(ui, "gain", 530.0, y + 6.0, "GAIN", false);
}

/// The hint line for a hovered target.
pub fn hint(t: &Target, m: &dyn Model, part: u8) -> String {
    match t {
        Target::Knob(id) | Target::Toggle(id) | Target::List(id) => format!("{}: {}", pretty(id), m.text(id)),
        Target::Set(id, _) => pretty(id).to_string(),
        Target::PartTab(p) => format!("Show part {}", if *p == 0 { "A" } else { "B" }),
        Target::Band(i) => format!("Spectrum band {}: {}", i + 1, m.text(&band_id(part, *i))),
    }
}

/// A readable name for a parameter id ("aa_band3" -> "Part A band 3").
pub fn pretty(id: &str) -> String {
    let (part, rest) = match id.split_once('_') {
        Some(("aa", r)) => ("Part A ", r),
        Some(("ab", r)) => ("Part B ", r),
        _ => ("", id),
    };
    format!("{part}{}", rest.replace('_', " "))
}
