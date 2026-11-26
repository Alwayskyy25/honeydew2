// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Pitch — a pitch-correction editor instrument in the style of FL Studio's Newtone, designed and written
// from scratch from public documentation of the original's feature set (see HARMOR-NEWTONE-RESEARCH.md in the
// Honeydew notes). No original code or assets are used.

//! Parameters: the three global correction knobs Newtone is built around — Center (how far each segment is
//! pulled to its target note), Variation (how much of the original pitch contour inside a segment is kept) and
//! Transition (how gradually the correction moves between segments) — plus master gain. Per-segment target
//! notes are edited in the editor and held in the shared analysis (see `dsp`).

use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;

fn percent() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{:.0} %", v * 100.0))
}

/// Scales the snap-to-scale correction can land on (index 0 = chromatic, i.e. every semitone).
pub const SCALES: [&str; 3] = ["Chromatic", "Major", "Minor"];
/// Pitch-class names for the scale root stepper.
pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Params)]
pub struct PitchParams {
    /// The loaded sample's path (empty = none).
    #[persist = "sample-path"]
    pub sample_path: Arc<RwLock<String>>,

    #[id = "center"]
    pub center: FloatParam,
    #[id = "variation"]
    pub variation: FloatParam,
    #[id = "transition"]
    pub transition: FloatParam,
    #[id = "snap"]
    pub snap: BoolParam,
    #[id = "scale"]
    pub scale: IntParam,
    #[id = "root"]
    pub root: IntParam,
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for PitchParams {
    fn default() -> Self {
        let knob = |name: &str, v: f32| {
            FloatParam::new(name, v, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(percent())
                .with_step_size(0.01)
        };
        Self {
            sample_path: Arc::new(RwLock::new(String::new())),
            center: knob("Center", 1.0),
            variation: knob("Variation", 0.5),
            transition: knob("Transition", 0.3),
            snap: BoolParam::new("Snap to scale", false),
            scale: IntParam::new(
                "Scale",
                1,
                IntRange::Linear {
                    min: 0,
                    max: SCALES.len() as i32 - 1,
                },
            )
            .with_value_to_string(Arc::new(|v| {
                SCALES[v.clamp(0, SCALES.len() as i32 - 1) as usize].to_string()
            })),
            root: IntParam::new("Scale root", 0, IntRange::Linear { min: 0, max: 11 })
                .with_value_to_string(Arc::new(|v| {
                    NOTE_NAMES[v.clamp(0, 11) as usize].to_string()
                })),
            gain: knob("Gain", 0.9),
        }
    }
}
