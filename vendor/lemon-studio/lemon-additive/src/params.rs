// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — an additive synthesizer in the style of FL Studio's Harmor, designed and written from scratch
// from public documentation of the original's feature set (see HARMOR-NEWTONE-RESEARCH.md in the Honeydew notes).
// No original code or assets are used.

//! Parameters, laid out the way Harmor's panel is: two independent parts (A and B), each a complete synthesis
//! bank — Timbre (two shapes blended by a mix knob), Blur, Prism, Harmonizer, Pluck, Phaser, two spectral
//! filters and a 3-band EQ — plus a drawable per-partial harmonic spectrum (the IMG section's analogue).
//! Globally: the part mix / selection mode, a volume ADSR, unison, vibrato, portamento / legato, strum, gain.
//!
//! A note's part in "Note channel" mode is chosen by its MIDI channel, the way FL Studio's piano roll note
//! colours choose Harmor's part: channel 1 (green) plays part A, channel 2 (red) plays part B.

use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;

/// Partials per part, and the resolution of the drawable spectrum (one gain per partial).
pub const PARTIALS: usize = 64;
/// Harmonic shapes a timbre slot starts from.
pub const SHAPES: [&str; 6] = ["Saw", "Square", "Triangle", "Sine", "Organ", "Bright"];
/// Spectral filter types for Filter 1 / Filter 2.
pub const FILTER_TYPES: [&str; 6] = ["Off", "Lowpass", "Highpass", "Bandpass", "Notch", "Peak"];
pub const FT_OFF: i32 = 0;
pub const FT_LOWPASS: i32 = 1;
pub const FT_HIGHPASS: i32 = 2;
pub const FT_BANDPASS: i32 = 3;
pub const FT_NOTCH: i32 = 4;
pub const FT_PEAK: i32 = 5;
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

/// A filter frequency knob (0..1) as Hz: exponential, 40 Hz .. 16 kHz.
pub fn filter_hz(n: f32) -> f32 {
    40.0 * (16000.0f32 / 40.0).powf(n.clamp(0.0, 1.0))
}

/// A blur time knob (0..1) as seconds: exponential, 1 ms .. 2 s.
pub fn blur_seconds(n: f32) -> f32 {
    0.001 * (2000.0f32).powf(n.clamp(0.0, 1.0))
}

fn percent() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{:.0} %", v * 100.0))
}
fn bipolar_percent() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{:+.0} %", v * 100.0))
}
fn cents() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{v:+.0} ct"))
}
fn db() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{:+.1} dB", v * 12.0))
}
fn hz() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| {
        let f = filter_hz(v);
        if f < 1000.0 {
            format!("{f:.0} Hz")
        } else {
            format!("{:.1} kHz", f / 1000.0)
        }
    })
}
fn seconds() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| {
        let s = env_seconds(v);
        if s < 1.0 {
            format!("{:.0} ms", s * 1000.0)
        } else {
            format!("{s:.2} s")
        }
    })
}

fn knob(name: &str, v: f32) -> FloatParam {
    FloatParam::new(name, v, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_value_to_string(percent())
        .with_step_size(0.01)
}
fn bipolar(name: &str, v: f32) -> FloatParam {
    FloatParam::new(
        name,
        v,
        FloatRange::Linear {
            min: -1.0,
            max: 1.0,
        },
    )
    .with_value_to_string(bipolar_percent())
    .with_step_size(0.01)
}

#[derive(Params)]
pub struct PartParams {
    #[id = "on"]
    pub on: BoolParam,
    #[id = "shape1"]
    pub shape1: IntParam,
    #[id = "shape2"]
    pub shape2: IntParam,
    #[id = "tmix"]
    pub tmix: FloatParam,
    #[id = "level"]
    pub level: FloatParam,
    #[id = "octave"]
    pub octave: IntParam,
    #[id = "detune"]
    pub detune: FloatParam,
    // Spectral shaping.
    #[id = "prism"]
    pub prism: FloatParam,
    #[id = "blur"]
    pub blur: FloatParam,
    #[id = "blurtime"]
    pub blur_time: FloatParam,
    #[id = "harm"]
    pub harm: FloatParam,
    #[id = "harmw"]
    pub harm_width: FloatParam,
    #[id = "pluck"]
    pub pluck: FloatParam,
    // Phaser.
    #[id = "phmix"]
    pub phaser_mix: FloatParam,
    #[id = "phwidth"]
    pub phaser_width: FloatParam,
    #[id = "phofs"]
    pub phaser_ofs: FloatParam,
    #[id = "phspeed"]
    pub phaser_speed: FloatParam,
    // Filter 1.
    #[id = "f1type"]
    pub f1_type: IntParam,
    #[id = "f1freq"]
    pub f1_freq: FloatParam,
    #[id = "f1res"]
    pub f1_res: FloatParam,
    #[id = "f1key"]
    pub f1_key: FloatParam,
    #[id = "f1env"]
    pub f1_env: FloatParam,
    // Filter 2.
    #[id = "f2type"]
    pub f2_type: IntParam,
    #[id = "f2freq"]
    pub f2_freq: FloatParam,
    #[id = "f2res"]
    pub f2_res: FloatParam,
    #[id = "f2key"]
    pub f2_key: FloatParam,
    #[id = "f2env"]
    pub f2_env: FloatParam,
    // EQ.
    #[id = "eqlow"]
    pub eq_low: FloatParam,
    #[id = "eqmid"]
    pub eq_mid: FloatParam,
    #[id = "eqhigh"]
    pub eq_high: FloatParam,
}

impl PartParams {
    fn new(on: bool, shape2: i32) -> Self {
        let shape = |name: &str, v: i32| {
            IntParam::new(
                name,
                v,
                IntRange::Linear {
                    min: 0,
                    max: SHAPES.len() as i32 - 1,
                },
            )
            .with_value_to_string(Arc::new(|v| {
                SHAPES[v.clamp(0, SHAPES.len() as i32 - 1) as usize].to_string()
            }))
        };
        let ftype = |name: &str, v: i32| {
            IntParam::new(
                name,
                v,
                IntRange::Linear {
                    min: 0,
                    max: FILTER_TYPES.len() as i32 - 1,
                },
            )
            .with_value_to_string(Arc::new(|v| {
                FILTER_TYPES[v.clamp(0, FILTER_TYPES.len() as i32 - 1) as usize].to_string()
            }))
        };
        let freq = |name: &str, v: f32| {
            FloatParam::new(name, v, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(hz())
                .with_step_size(0.01)
        };
        let eq = |name: &str| {
            FloatParam::new(
                name,
                0.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .with_value_to_string(db())
            .with_step_size(0.01)
        };
        Self {
            on: BoolParam::new("Part on", on),
            shape1: shape("Timbre 1", 0),
            shape2: shape("Timbre 2", shape2),
            tmix: knob("Timbre mix", 0.0),
            level: knob("Part level", 0.8),
            octave: IntParam::new("Octave", 0, IntRange::Linear { min: -2, max: 2 })
                .with_value_to_string(Arc::new(|v| format!("{v:+} oct"))),
            detune: FloatParam::new(
                "Detune",
                0.0,
                FloatRange::Linear {
                    min: -100.0,
                    max: 100.0,
                },
            )
            .with_value_to_string(cents())
            .with_step_size(1.0),
            prism: knob("Prism", 0.0),
            blur: knob("Blur mix", 0.0),
            blur_time: FloatParam::new(
                "Blur time",
                0.35,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| {
                let s = blur_seconds(v);
                if s < 1.0 {
                    format!("{:.0} ms", s * 1000.0)
                } else {
                    format!("{s:.2} s")
                }
            }))
            .with_step_size(0.01),
            harm: knob("Harmonizer", 0.0),
            harm_width: knob("Harmonizer width", 0.0),
            pluck: knob("Pluck", 0.0),
            phaser_mix: knob("Phaser mix", 0.0),
            phaser_width: knob("Phaser width", 0.5),
            phaser_ofs: freq("Phaser offset", 0.45),
            phaser_speed: FloatParam::new(
                "Phaser speed",
                0.25,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| format!("{:.2} Hz", 0.05 + v * v * 8.0)))
            .with_step_size(0.01),
            f1_type: ftype("Filter 1 type", FT_LOWPASS),
            f1_freq: freq("Filter 1 freq", 0.85),
            f1_res: knob("Filter 1 res", 0.0),
            f1_key: knob("Filter 1 key track", 0.0),
            f1_env: bipolar("Filter 1 env", 0.0),
            f2_type: ftype("Filter 2 type", FT_OFF),
            f2_freq: freq("Filter 2 freq", 0.85),
            f2_res: knob("Filter 2 res", 0.0),
            f2_key: knob("Filter 2 key track", 0.0),
            f2_env: bipolar("Filter 2 env", 0.0),
            eq_low: eq("EQ low"),
            eq_mid: eq("EQ mid"),
            eq_high: eq("EQ high"),
        }
    }
}

#[derive(Params)]
pub struct AdditiveParams {
    #[nested(group = "Part A", id_prefix = "aa")]
    pub a: PartParams,
    #[nested(group = "Part B", id_prefix = "ab")]
    pub b: PartParams,

    /// Part A's drawable harmonic spectrum: one gain per partial (persisted with the patch).
    #[persist = "spec-a"]
    pub spec_a: Arc<RwLock<Vec<f32>>>,
    /// Part B's drawable harmonic spectrum.
    #[persist = "spec-b"]
    pub spec_b: Arc<RwLock<Vec<f32>>>,

    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "partmode"]
    pub part_mode: IntParam,
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
    #[id = "upan"]
    pub upan: FloatParam,
    #[id = "vibdepth"]
    pub vib_depth: FloatParam,
    #[id = "vibspeed"]
    pub vib_speed: FloatParam,
    #[id = "porta"]
    pub porta: FloatParam,
    #[id = "legato"]
    pub legato: BoolParam,
    #[id = "strum"]
    pub strum: FloatParam,
    #[id = "gain"]
    pub gain: FloatParam,
}

impl AdditiveParams {
    /// The drawable spectrum of part `p` (0 = A, 1 = B): PARTIALS gains, missing entries read as 1.
    pub fn spectrum(&self, p: usize) -> Vec<f32> {
        let src = if p == 0 { &self.spec_a } else { &self.spec_b };
        let mut out = vec![1.0f32; PARTIALS];
        if let Ok(v) = src.read() {
            for (o, s) in out.iter_mut().zip(v.iter()) {
                *o = s.clamp(0.0, 1.0);
            }
        }
        out
    }

    /// Sets one partial gain of part `p`'s spectrum (the editor's draw gesture).
    pub fn set_spectrum(&self, p: usize, k: usize, v: f32) {
        let src = if p == 0 { &self.spec_a } else { &self.spec_b };
        if let Ok(mut g) = src.write() {
            if g.len() != PARTIALS {
                g.resize(PARTIALS, 1.0);
            }
            if k < PARTIALS {
                g[k] = v.clamp(0.0, 1.0);
            }
        }
    }

    /// Replaces part `p`'s whole spectrum (preset buttons).
    pub fn set_spectrum_all(&self, p: usize, vals: &[f32]) {
        let src = if p == 0 { &self.spec_a } else { &self.spec_b };
        if let Ok(mut g) = src.write() {
            g.clear();
            g.extend(vals.iter().take(PARTIALS).map(|v| v.clamp(0.0, 1.0)));
            g.resize(PARTIALS, 1.0);
        }
    }
}

impl Default for AdditiveParams {
    fn default() -> Self {
        let time = |name: &str, v: f32| {
            FloatParam::new(name, v, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(seconds())
                .with_step_size(0.01)
        };
        Self {
            a: PartParams::new(true, 1),
            b: PartParams::new(false, 1),
            spec_a: Arc::new(RwLock::new(vec![1.0; PARTIALS])),
            spec_b: Arc::new(RwLock::new(vec![1.0; PARTIALS])),
            mix: knob("Part mix", 0.5),
            part_mode: IntParam::new("Part mode", MODE_MIX, IntRange::Linear { min: 0, max: 3 })
                .with_value_to_string(Arc::new(|v| PART_MODES[v.clamp(0, 3) as usize].to_string())),
            attack: time("Attack", 0.02),
            decay: time("Decay", 0.25),
            sustain: knob("Sustain", 0.8),
            release: time("Release", 0.2),
            unison: IntParam::new("Unison voices", 1, IntRange::Linear { min: 1, max: 4 }),
            udetune: FloatParam::new(
                "Unison detune",
                8.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 50.0,
                },
            )
            .with_value_to_string(cents())
            .with_step_size(0.5),
            upan: knob("Unison pan", 0.5),
            vib_depth: FloatParam::new(
                "Vibrato depth",
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_value_to_string(Arc::new(|v| format!("{v:.0} ct")))
            .with_step_size(1.0),
            vib_speed: FloatParam::new(
                "Vibrato speed",
                0.3,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(Arc::new(|v| format!("{:.1} Hz", 0.5 + v * 9.5)))
            .with_step_size(0.01),
            porta: FloatParam::new("Portamento", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| {
                    let s = v * v * 0.5;
                    if s < 0.001 {
                        "Off".to_string()
                    } else {
                        format!("{:.0} ms", s * 1000.0)
                    }
                }))
                .with_step_size(0.01),
            legato: BoolParam::new("Legato", false),
            strum: FloatParam::new("Strum", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(Arc::new(|v| format!("{:.0} ms", v * 60.0)))
                .with_step_size(0.01),
            gain: knob("Gain", 0.8),
        }
    }
}
