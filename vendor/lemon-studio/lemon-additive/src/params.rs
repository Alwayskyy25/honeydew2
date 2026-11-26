// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — an additive synthesizer in the style of FL Studio's Harmor, designed and written from scratch
// from public documentation of the original's feature set (see HARMOR-NEWTONE-RESEARCH.md in the Honeydew notes).
// No original code or assets are used.

//! Parameters: two independent parts (A and B), each with a harmonic shape, a drawable 12-band spectrum, level,
//! octave and detune; global part mix / selection mode, brightness (spectral tilt), prism (inharmonicity),
//! a volume ADSR, unison and master gain.
//!
//! A note's part in "Note channel" mode is chosen by its MIDI channel, the way FL Studio's piano roll note
//! colours choose Harmor's part: channel 1 (green) plays part A, channel 2 (red) plays part B.

use std::sync::Arc;

use nih_plug::prelude::*;

/// Spectrum bands per part (log-spaced over the partial index).
pub const BANDS: usize = 12;
/// Harmonic shapes a part starts from (the band curve multiplies the shape's spectrum).
pub const SHAPES: [&str; 6] = ["Saw", "Square", "Triangle", "Sine", "Organ", "Bright"];
/// Part selection modes: mix both parts, A only, B only, or per-note by MIDI channel.
pub const PART_MODES: [&str; 4] = ["Mix A+B", "Part A", "Part B", "Note channel"];
pub const MODE_MIX: i32 = 0;
pub const MODE_A: i32 = 1;
pub const MODE_B: i32 = 2;
pub const MODE_CHANNEL: i32 = 3;

/// An envelope time knob (0..1) as seconds: a cubic law, 2 ms .. ~8 s.
pub fn env_seconds(n: f32) -> f32 {
    0.002 + n.clamp(0.0, 1.0).powi(3) * 8.0
}

fn percent() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{:.0} %", v * 100.0))
}
fn cents() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{v:+.0} ct"))
}
fn seconds() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| {
        let s = env_seconds(v);
        if s < 1.0 { format!("{:.0} ms", s * 1000.0) } else { format!("{s:.2} s") }
    })
}

#[derive(Params)]
pub struct PartParams {
    #[id = "on"]
    pub on: BoolParam,
    #[id = "shape"]
    pub shape: IntParam,
    #[id = "level"]
    pub level: FloatParam,
    #[id = "octave"]
    pub octave: IntParam,
    #[id = "detune"]
    pub detune: FloatParam,
    #[id = "band1"]
    pub band1: FloatParam,
    #[id = "band2"]
    pub band2: FloatParam,
    #[id = "band3"]
    pub band3: FloatParam,
    #[id = "band4"]
    pub band4: FloatParam,
    #[id = "band5"]
    pub band5: FloatParam,
    #[id = "band6"]
    pub band6: FloatParam,
    #[id = "band7"]
    pub band7: FloatParam,
    #[id = "band8"]
    pub band8: FloatParam,
    #[id = "band9"]
    pub band9: FloatParam,
    #[id = "band10"]
    pub band10: FloatParam,
    #[id = "band11"]
    pub band11: FloatParam,
    #[id = "band12"]
    pub band12: FloatParam,
}

impl PartParams {
    fn new(on: bool) -> Self {
        let band = |name: &str| {
            FloatParam::new(name, 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01)
        };
        Self {
            on: BoolParam::new("Part on", on),
            shape: IntParam::new("Timbre shape", 0, IntRange::Linear { min: 0, max: SHAPES.len() as i32 - 1 })
                .with_value_to_string(Arc::new(|v| SHAPES[v.clamp(0, SHAPES.len() as i32 - 1) as usize].to_string())),
            level: FloatParam::new("Part level", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01),
            octave: IntParam::new("Octave", 0, IntRange::Linear { min: -2, max: 2 })
                .with_value_to_string(Arc::new(|v| format!("{v:+} oct"))),
            detune: FloatParam::new("Detune", 0.0, FloatRange::Linear { min: -100.0, max: 100.0 })
                .with_value_to_string(cents())
                .with_step_size(1.0),
            band1: band("Band 1"),
            band2: band("Band 2"),
            band3: band("Band 3"),
            band4: band("Band 4"),
            band5: band("Band 5"),
            band6: band("Band 6"),
            band7: band("Band 7"),
            band8: band("Band 8"),
            band9: band("Band 9"),
            band10: band("Band 10"),
            band11: band("Band 11"),
            band12: band("Band 12"),
        }
    }

    /// The 12 band gains in order.
    pub fn bands(&self) -> [f32; BANDS] {
        [
            self.band1.value(),
            self.band2.value(),
            self.band3.value(),
            self.band4.value(),
            self.band5.value(),
            self.band6.value(),
            self.band7.value(),
            self.band8.value(),
            self.band9.value(),
            self.band10.value(),
            self.band11.value(),
            self.band12.value(),
        ]
    }
}

#[derive(Params)]
pub struct AdditiveParams {
    #[nested(group = "Part A", id_prefix = "aa")]
    pub a: PartParams,
    #[nested(group = "Part B", id_prefix = "ab")]
    pub b: PartParams,

    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "partmode"]
    pub part_mode: IntParam,
    #[id = "bright"]
    pub bright: FloatParam,
    #[id = "prism"]
    pub prism: FloatParam,
    #[id = "attack"]
    pub attack: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "sustain"]
    pub sustain: FloatParam,
    #[id = "release"]
    pub release: FloatParam,
    #[id = "unison"]
    pub unison: IntParam,
    #[id = "udetune"]
    pub udetune: FloatParam,
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for AdditiveParams {
    fn default() -> Self {
        let time = |name: &str, v: f32| {
            FloatParam::new(name, v, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(seconds())
                .with_step_size(0.01)
        };
        Self {
            a: PartParams::new(true),
            b: PartParams::new(false),
            mix: FloatParam::new("Part mix", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01),
            part_mode: IntParam::new("Part mode", MODE_MIX, IntRange::Linear { min: 0, max: 3 })
                .with_value_to_string(Arc::new(|v| PART_MODES[v.clamp(0, 3) as usize].to_string())),
            bright: FloatParam::new("Brightness", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01),
            prism: FloatParam::new("Prism", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01),
            attack: time("Attack", 0.02),
            decay: time("Decay", 0.25),
            sustain: FloatParam::new("Sustain", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01),
            release: time("Release", 0.2),
            unison: IntParam::new("Unison voices", 1, IntRange::Linear { min: 1, max: 4 }),
            udetune: FloatParam::new("Unison detune", 8.0, FloatRange::Linear { min: 0.0, max: 50.0 })
                .with_value_to_string(cents())
                .with_step_size(0.5),
            gain: FloatParam::new("Gain", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01),
        }
    }
}
