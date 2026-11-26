// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — original implementation (see params.rs note).

//! Lemon Additive editor painting and hit regions (platform independent), laid out like Harmor's panel:
//! the part row and timbre blend across the top, the drawable harmonic spectrum (the IMG section's analogue)
//! under it, then the per-part sections — Spectral (prism / blur / harmonizer / pluck), Filter 1, Filter 2
//! and EQ — and the global envelope / pitch / unison rows along the bottom.

use os_theme::Palette;
use os_theme::design::lemon as brand;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{self, State, inst};
use os_ui::{Canvas, Color, Font, Rect};

use crate::params::{FILTER_TYPES, PARTIALS, SHAPES};

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
    /// One partial of the shown part's drawable spectrum (drag to paint levels).
    Spec(usize),
    /// A spectrum preset button: 0 flat, 1 decay, 2 odd only, 3 clear.
    SpecPreset(u8),
    /// Copy the shown part's settings onto the other part.
    CopyPart,
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
        ViewState {
            part: 0,
            mouse: None,
            pressed: None,
        }
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

/// The shown part's parameter id prefix.
pub fn prefix(part: u8) -> &'static str {
    if part == 0 { "aa" } else { "ab" }
}

/// The spectrum well's rectangle.
pub fn spectrum_rect() -> Rect {
    Rect::new(16.0, 134.0, 536.0, 96.0)
}

/// The x centre of partial `k`'s column (0-based) in the spectrum well.
pub fn spec_x(k: usize) -> f32 {
    let a = spectrum_rect();
    a.x + (k as f32 + 0.5) * a.w / PARTIALS as f32
}

/// The partial index at painted x (clamped).
pub fn spec_at_x(x: f32) -> usize {
    let a = spectrum_rect();
    (((x - a.x) / a.w * PARTIALS as f32) as i32).clamp(0, PARTIALS as i32 - 1) as usize
}

/// The spectrum level at painted y (0 at the well's bottom, 1 at its top).
pub fn spec_value_at_y(y: f32) -> f32 {
    let a = spectrum_rect();
    ((a.y + a.h - y) / a.h).clamp(0.0, 1.0)
}

/// Paints the editor. `spec` is the shown part's drawable spectrum. `hosted` = the host's strip row
/// carries the page tab and the hint line.
pub fn paint(
    c: &mut Canvas,
    t: &Palette,
    m: &dyn Model,
    spec: &[f32],
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
        ui.c.text(TITLE, 16.0, 34.0, brand::TEXT, "LEMON ADDITIVE");
        if !hint.is_empty() {
            ui.c.text_clip(LABEL, 190.0, 36.0, 360.0, plug::DIM, hint);
        }
    }
    part_row(&mut ui);
    timbre_row(&mut ui);
    spectrum(&mut ui, spec);
    sections(&mut ui);
    global_rows(&mut ui);
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

/// A - value + stepper for an int parameter.
fn stepper(ui: &mut Ui, id: &str, x: f32, y: f32, w: f32, label: &str, min: i32, max: i32) {
    let cur = ui.m.plain(id).round() as i32;
    button(
        ui,
        rr(x, y, 20.0, 20.0),
        "-",
        false,
        Target::Set(id.into(), (cur - 1).max(min) as f32),
        brand::MAIN,
    );
    ui.c.text_center(
        LABEL,
        rr(x + 22.0, y + 2.0, w - 44.0, 16.0),
        brand::TEXT,
        label,
    );
    button(
        ui,
        rr(x + w - 20.0, y, 20.0, 20.0),
        "+",
        false,
        Target::Set(id.into(), (cur + 1).min(max) as f32),
        brand::MAIN,
    );
}

/// The part selector row: PART A / PART B tabs, the shown part's on switch, copy, octave, part mode.
fn part_row(ui: &mut Ui) {
    let y = 54.0;
    for p in 0..2u8 {
        let r = rr(16.0 + p as f32 * 108.0, y, 102.0, 24.0);
        let col = Color::rgb(PART_COLORS[p as usize]);
        button(
            ui,
            r,
            if p == 0 { "PART A" } else { "PART B" },
            ui.st.part == p,
            Target::PartTab(p),
            col,
        );
    }
    let pre = prefix(ui.st.part);
    let col = Color::rgb(PART_COLORS[ui.st.part as usize]);
    let on_id = format!("{pre}_on");
    let on = ui.m.plain(&on_id) > 0.5;
    button(
        ui,
        rr(238.0, y, 52.0, 24.0),
        if on { "ON" } else { "OFF" },
        on,
        Target::Toggle(on_id),
        col,
    );
    button(
        ui,
        rr(296.0, y, 62.0, 24.0),
        if ui.st.part == 0 {
            "COPY A>B"
        } else {
            "COPY B>A"
        },
        false,
        Target::CopyPart,
        brand::MAIN,
    );
    // Octave stepper.
    let oct_id = format!("{pre}_octave");
    let oct = ui.m.plain(&oct_id).round() as i32;
    ui.c.text(CAP, 366.0, y + 4.0, plug::DIM, "OCT");
    stepper(
        ui,
        &oct_id,
        390.0,
        y + 2.0,
        76.0,
        &format!("{oct:+}"),
        -2,
        2,
    );
    // Part mode cycles on click.
    let mode = ui.m.plain("partmode").round() as i32;
    let mode_label = ["MIX A+B", "PART A", "PART B", "NOTE CH"][mode.clamp(0, 3) as usize];
    button(
        ui,
        rr(474.0, y, 78.0, 24.0),
        mode_label,
        false,
        Target::Set("partmode".into(), ((mode + 1) % 4) as f32),
        brand::MAIN,
    );
}

/// The timbre row: the two shape steppers and the mix knob (Harmor's Timbre 1 / Timbre 2 / MIX).
fn timbre_row(ui: &mut Ui) {
    let pre = prefix(ui.st.part);
    ui.c.text(CAP, 16.0, 92.0, plug::DIM, "TIMBRE");
    for (slot, x) in [(1usize, 70.0f32), (2, 286.0)] {
        let id = format!("{pre}_shape{slot}");
        let cur = ui.m.plain(&id).round() as i32;
        ui.c.text(CAP, x, 92.0, plug::DIM, if slot == 1 { "1" } else { "2" });
        stepper(
            ui,
            &id,
            x + 12.0,
            86.0,
            190.0,
            SHAPES[cur.clamp(0, 5) as usize],
            0,
            5,
        );
    }
    knob(ui, &format!("{pre}_tmix"), 524.0, 96.0, 11.0, "MIX", false);
}

/// The shape recipe, mirrored from the DSP for display.
fn shape_amp(shape: i32, k: usize) -> f32 {
    let kf = k as f32;
    match shape {
        0 => 1.0 / kf,
        1 => {
            if k % 2 == 1 {
                1.0 / kf
            } else {
                0.0
            }
        }
        2 => {
            if k % 2 == 1 {
                1.0 / (kf * kf)
            } else {
                0.0
            }
        }
        3 => {
            if k == 1 {
                1.0
            } else {
                0.0
            }
        }
        4 => match k {
            1 | 2 | 3 | 4 | 6 | 8 => 1.0 / kf.sqrt(),
            _ => 0.0,
        },
        _ => 1.0 / kf.sqrt(),
    }
}

/// The drawable harmonic spectrum: one column per partial; the bar is the drawn level, the bright cap
/// shows the resulting amplitude after the timbre blend.
fn spectrum(ui: &mut Ui, spec: &[f32]) {
    let area = spectrum_rect();
    ui.c.text(
        CAP,
        area.x + 2.0,
        area.y - 14.0,
        plug::DIM,
        "HARMONICS — drag to draw the partial levels",
    );
    let presets: [(&str, u8); 4] = [("FLAT", 0), ("DECAY", 1), ("ODD", 2), ("CLEAR", 3)];
    for (i, (label, id)) in presets.iter().enumerate() {
        let r = rr(364.0 + i as f32 * 48.0, area.y - 18.0, 44.0, 16.0);
        button(ui, r, label, false, Target::SpecPreset(*id), brand::MAIN);
    }
    ui.c.fill_rrect(area, 6.0, plug::WELL);
    let pre = prefix(ui.st.part);
    let s1 = ui.m.plain(&format!("{pre}_shape1")).round() as i32;
    let s2 = ui.m.plain(&format!("{pre}_shape2")).round() as i32;
    let tm = ui.m.norm(&format!("{pre}_tmix"));
    let col = Color::rgb(PART_COLORS[ui.st.part as usize]);
    let cw = area.w / PARTIALS as f32;
    for k in 0..PARTIALS {
        let v = spec.get(k).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        let x = area.x + k as f32 * cw;
        let bh = v * (area.h - 12.0);
        ui.c.fill_rrect(
            rr(x + 1.0, area.y + area.h - 6.0 - bh, cw - 2.0, bh.max(1.5)),
            2.0,
            col.with_alpha(110),
        );
        // Resulting amplitude cap (timbre blend x drawn level), scaled so a saw fundamental reaches the top.
        let blend = shape_amp(s1, k + 1) * (1.0 - tm) + shape_amp(s2, k + 1) * tm;
        let res = (blend.abs() * v).clamp(0.0, 1.0);
        let ry = area.y + area.h - 6.0 - res * (area.h - 12.0);
        ui.c.fill_rect(rr(x + 1.0, ry - 1.5, cw - 2.0, 3.0), col);
        ui.hit(rr(x, area.y, cw, area.h), Target::Spec(k));
    }
}

/// A lemon knob with its caption and value.
fn knob(ui: &mut Ui, id: &str, cx: f32, cy: f32, r: f32, cap: &str, bipolar: bool) {
    let target = Target::Knob(id.into());
    let hr = rr(cx - r - 7.0, cy - r - 7.0, (r + 7.0) * 2.0, (r + 7.0) * 2.0);
    let st = ui.state(hr, &target);
    lemon::knob(
        ui.c,
        ui.t,
        cx,
        cy,
        r,
        ui.m.norm(id),
        bipolar,
        kit::KnobLook::Ball,
        st,
    );
    ui.c.text_center(CAP, rr(cx - 40.0, cy + r + 3.0, 80.0, 12.0), plug::DIM, cap);
    ui.hit(hr, target);
}

/// One labelled section column of small knobs (two rows of three).
fn section(ui: &mut Ui, x: f32, title: &str, knobs: [(&str, &str, bool); 6]) {
    ui.c.text(CAP, x + 2.0, 244.0, plug::DIM, title);
    for (i, (id, cap, bi)) in knobs.iter().enumerate() {
        let cx = x + 30.0 + (i % 3) as f32 * 58.0;
        let cy = if i < 3 { 272.0 } else { 318.0 };
        knob(ui, id, cx, cy, 10.0, cap, *bi);
    }
}

/// The per-part sections: Spectral | Filter 1 + part level/detune | Filter 2 + EQ.
fn sections(ui: &mut Ui) {
    let pre = prefix(ui.st.part);
    let f = |name: &str| format!("{pre}_{name}");
    section(
        ui,
        16.0,
        "SPECTRAL",
        [
            (&f("prism"), "PRISM", false),
            (&f("blur"), "BLUR", false),
            (&f("blurtime"), "B-TIME", false),
            (&f("harm"), "HARM", false),
            (&f("harmw"), "H-WIDTH", false),
            (&f("pluck"), "PLUCK", false),
        ],
    );
    // Filter type steppers sit in the section title rows.
    section(
        ui,
        196.0,
        "FILTER 1 · PART",
        [
            (&f("f1freq"), "F1 FREQ", false),
            (&f("f1res"), "F1 RES", false),
            (&f("f1key"), "F1 KEY", false),
            (&f("f1env"), "F1 ENV", true),
            (&f("level"), "LEVEL", false),
            (&f("detune"), "DETUNE", true),
        ],
    );
    section(
        ui,
        376.0,
        "FILTER 2 · EQ",
        [
            (&f("f2freq"), "F2 FREQ", false),
            (&f("f2res"), "F2 RES", false),
            (&f("f2env"), "F2 ENV", true),
            (&f("eqlow"), "EQ LOW", true),
            (&f("eqmid"), "EQ MID", true),
            (&f("eqhigh"), "EQ HIGH", true),
        ],
    );
    for (slot, x) in [(1usize, 196.0f32), (2, 376.0)] {
        let id = f(&format!("f{slot}type"));
        let cur = ui.m.plain(&id).round() as i32;
        stepper(
            ui,
            &id,
            x + 88.0,
            238.0,
            88.0,
            FILTER_TYPES[cur.clamp(0, 5) as usize],
            0,
            5,
        );
    }
    // Phaser row of the shown part, right under the sections.
    ui.c.text(CAP, 18.0, 348.0, plug::DIM, "PHASER");
    knob(ui, &f("phmix"), 84.0, 344.0, 10.0, "MIX", false);
    knob(ui, &f("phwidth"), 142.0, 344.0, 10.0, "WIDTH", false);
    knob(ui, &f("phofs"), 200.0, 344.0, 10.0, "OFFSET", false);
    knob(ui, &f("phspeed"), 258.0, 344.0, 10.0, "SPEED", false);
}

/// The global rows: envelope + mix/gain, then vibrato / portamento / strum / legato / unison.
fn global_rows(ui: &mut Ui) {
    ui.c.text(CAP, 330.0, 348.0, plug::DIM, "ENVELOPE");
    knob(ui, "attack", 380.0, 344.0, 10.0, "ATTACK", false);
    knob(ui, "decay", 428.0, 344.0, 10.0, "DECAY", false);
    knob(ui, "sustain", 476.0, 344.0, 10.0, "SUSTAIN", false);
    knob(ui, "release", 524.0, 344.0, 10.0, "RELEASE", false);
    let y = 404.0;
    knob(ui, "mix", 36.0, y, 11.0, "MIX A/B", false);
    knob(ui, "gain", 92.0, y, 11.0, "GAIN", false);
    knob(ui, "vibdepth", 148.0, y, 11.0, "VIB DEPTH", false);
    knob(ui, "vibspeed", 204.0, y, 11.0, "VIB SPEED", false);
    knob(ui, "porta", 260.0, y, 11.0, "PORTA", false);
    knob(ui, "strum", 316.0, y, 11.0, "STRUM", false);
    knob(ui, "udetune", 372.0, y, 11.0, "U-DETUNE", false);
    knob(ui, "upan", 428.0, y, 11.0, "U-PAN", false);
    let u = ui.m.plain("unison").round() as i32;
    stepper(ui, "unison", 478.0, 366.0, 74.0, &format!("UNI {u}"), 1, 4);
    let leg = ui.m.plain("legato") > 0.5;
    button(
        ui,
        rr(478.0, y - 12.0, 74.0, 24.0),
        "LEGATO",
        leg,
        Target::Toggle("legato".into()),
        brand::MAIN,
    );
}

/// The hint line for a hovered target.
pub fn hint(t: &Target, m: &dyn Model, part: u8) -> String {
    match t {
        Target::Knob(id) | Target::Toggle(id) | Target::List(id) => {
            format!("{}: {}", pretty(id), m.text(id))
        }
        Target::Set(id, _) => pretty(id).to_string(),
        Target::PartTab(p) => format!("Show part {}", if *p == 0 { "A" } else { "B" }),
        Target::Spec(k) => format!(
            "Partial {} of part {} — drag to draw its level",
            k + 1,
            if part == 0 { "A" } else { "B" }
        ),
        Target::SpecPreset(p) => [
            "Spectrum: flat (neutral)",
            "Spectrum: decay slope",
            "Spectrum: odd partials only",
            "Spectrum: clear",
        ][*p as usize]
            .to_string(),
        Target::CopyPart => format!(
            "Copy part {} onto part {}",
            if part == 0 { "A" } else { "B" },
            if part == 0 { "B" } else { "A" }
        ),
    }
}

/// A readable name for a parameter id ("aa_f1freq" -> "Part A f1freq").
pub fn pretty(id: &str) -> String {
    let (part, rest) = match id.split_once('_') {
        Some(("aa", r)) => ("Part A ", r),
        Some(("ab", r)) => ("Part B ", r),
        _ => ("", id),
    };
    format!("{part}{}", rest.replace('_', " "))
}
