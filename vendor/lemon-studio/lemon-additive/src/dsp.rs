// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — original implementation (see params.rs note).

//! The additive engine: each voice sums up to [`MAX_PARTIALS`] sine partials per part. A part's spectrum is its
//! shape's harmonic recipe multiplied by the drawable 12-band curve and a brightness tilt; the prism control
//! stretches partial frequencies slightly inharmonic, the way Harmor's prism does. Voices carry a per-part phase
//! bank so parts with different octaves/detunes stay independent.

use std::sync::OnceLock;

use crate::params::{AdditiveParams, PartParams, BANDS, MODE_A, MODE_B, MODE_CHANNEL};

/// Partials summed per part (Harmor uses up to 516; 64 keeps a phone CPU comfortable and covers most timbres).
pub const MAX_PARTIALS: usize = 64;
/// Voices including unison copies.
const MAX_VOICES: usize = 24;
const TABLE: usize = 2048;

fn sine_table() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| (0..=TABLE).map(|i| (i as f32 / TABLE as f32 * std::f32::consts::TAU).sin()).collect())
}

/// Sine of a phase in cycles (0..1), table + linear interpolation.
fn sine(phase: f32) -> f32 {
    let t = sine_table();
    let x = phase.fract() * TABLE as f32;
    let i = x as usize % TABLE;
    let f = x - x.floor();
    t[i] * (1.0 - f) + t[i + 1] * f
}

/// The harmonic recipe of a shape: the (signed) amplitude of partial `k` (1-based) before bands and tilt.
fn shape_amp(shape: i32, k: usize) -> f32 {
    let kf = k as f32;
    match shape {
        // Saw
        0 => 1.0 / kf,
        // Square
        1 => {
            if k % 2 == 1 { 1.0 / kf } else { 0.0 }
        }
        // Triangle
        2 => {
            if k % 2 == 1 {
                let s = if (k / 2) % 2 == 0 { 1.0 } else { -1.0 };
                s / (kf * kf)
            } else {
                0.0
            }
        }
        // Sine
        3 => {
            if k == 1 { 1.0 } else { 0.0 }
        }
        // Organ: the first drawbar harmonics
        4 => match k {
            1 | 2 | 3 | 4 | 6 | 8 => 1.0 / kf.sqrt(),
            _ => 0.0,
        },
        // Bright: slow rolloff
        _ => 1.0 / kf.sqrt(),
    }
}

/// The band curve's gain at partial `k`: bands are log-spaced over the partial index, linearly interpolated.
fn band_gain(bands: &[f32; BANDS], k: usize) -> f32 {
    if k <= 1 {
        return bands[0];
    }
    let x = (k as f32).ln() / (MAX_PARTIALS as f32).ln() * (BANDS - 1) as f32;
    let i = (x as usize).min(BANDS - 2);
    let f = x - i as f32;
    bands[i] * (1.0 - f) + bands[i + 1] * f
}

/// One part's per-block tables: partial amplitudes and frequency multipliers (prism applied), plus the count
/// of partials that fit under Nyquist for fundamental `f0`.
struct PartTables {
    amps: [f32; MAX_PARTIALS],
    mults: [f32; MAX_PARTIALS],
    count: usize,
}

fn part_tables(part: &PartParams, bright: f32, prism: f32, f0: f32, sr: f32) -> PartTables {
    let shape = part.shape.value();
    let bands = part.bands();
    let tilt = (0.5 - bright) * -1.4; // bright 0 -> highs cut, 0.5 -> flat, 1 -> highs boosted
    let mut amps = [0.0f32; MAX_PARTIALS];
    let mut mults = [0.0f32; MAX_PARTIALS];
    let mut sum = 0.0f32;
    let mut count = 0usize;
    for k in 1..=MAX_PARTIALS {
        let kf = k as f32;
        if f0 * kf >= sr * 0.45 {
            break;
        }
        count = k;
        mults[k - 1] = kf.powf(1.0 + prism * 0.02);
        let a = shape_amp(shape, k) * band_gain(&bands, k) * kf.powf(tilt);
        amps[k - 1] = a;
        sum += a.abs();
    }
    let norm = if sum > 1.0 { 1.0 / sum } else { 1.0 };
    for a in amps.iter_mut() {
        *a *= norm;
    }
    PartTables { amps, mults, count }
}

#[derive(Clone, Copy, PartialEq)]
enum EnvPhase {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

struct Voice {
    note: u8,
    channel: u8,
    vel: f32,
    /// Which parts sound for this note (decided at note-on by the part mode / the note's channel).
    parts: [bool; 2],
    /// Unison detune offset in cents.
    uni_cents: f32,
    phases: [[f32; MAX_PARTIALS]; 2],
    env: EnvPhase,
    env_level: f32,
    age: u64,
}

impl Voice {
    /// Advances the envelope one sample; returns the gain. `att`/`dec`/`rel` in samples, `sus` the sustain level.
    fn tick_env(&mut self, att: f32, dec: f32, sus: f32, rel: f32) -> f32 {
        match self.env {
            EnvPhase::Attack => {
                self.env_level += 1.0 / att.max(1.0);
                if self.env_level >= 1.0 {
                    self.env_level = 1.0;
                    self.env = EnvPhase::Decay;
                }
            }
            EnvPhase::Decay => {
                let rate = (1.0 - sus) / dec.max(1.0);
                self.env_level -= rate;
                if self.env_level <= sus {
                    self.env_level = sus;
                    self.env = EnvPhase::Sustain;
                }
            }
            EnvPhase::Sustain => {}
            EnvPhase::Release => {
                self.env_level -= 1.0 / rel.max(1.0) * self.env_level.max(0.05);
                if self.env_level <= 0.001 {
                    self.env_level = 0.0;
                    self.env = EnvPhase::Done;
                }
            }
            EnvPhase::Done => {}
        }
        self.env_level
    }
}

/// The synth engine: voices, note handling and rendering.
pub struct Engine {
    sr: f32,
    voices: Vec<Voice>,
    age: u64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Engine { sr: 44100.0, voices: Vec::new(), age: 0 }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        if sr > 0.0 {
            self.sr = sr;
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    pub fn note_on(&mut self, params: &AdditiveParams, note: u8, channel: u8, velocity: f32) {
        // A retriggered note releases its old voices first.
        self.note_off(note, channel);
        let parts = match params.part_mode.value() {
            MODE_A => [params.a.on.value(), false],
            MODE_B => [false, params.b.on.value()],
            MODE_CHANNEL => {
                // Piano roll colours in FL: channel 1 (green) = part A, channel 2 (red) = part B.
                if channel == 1 { [false, true] } else { [true, false] }
            }
            _ => [params.a.on.value(), params.b.on.value()],
        };
        if !parts[0] && !parts[1] {
            return;
        }
        let u = params.unison.value().clamp(1, 4) as usize;
        let spread = params.udetune.value();
        for i in 0..u {
            if self.voices.len() >= MAX_VOICES {
                // Steal the oldest voice.
                if let Some((at, _)) = self.voices.iter().enumerate().min_by_key(|(_, v)| v.age) {
                    self.voices.remove(at);
                }
            }
            let uni_cents = if u == 1 { 0.0 } else { spread * (i as f32 * 2.0 / (u - 1) as f32 - 1.0) };
            self.age += 1;
            self.voices.push(Voice {
                note,
                channel,
                vel: velocity.clamp(0.0, 1.0),
                parts,
                uni_cents,
                phases: [[0.0; MAX_PARTIALS]; 2],
                env: EnvPhase::Attack,
                env_level: 0.0,
                age: self.age,
            });
        }
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        for v in self.voices.iter_mut() {
            if v.note == note && v.channel == channel && v.env != EnvPhase::Release {
                v.env = EnvPhase::Release;
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        self.voices.clear();
    }

    /// Renders (adds into) stereo buffers. `bend` is the pitch bend in semitones.
    pub fn render(&mut self, params: &AdditiveParams, bend: f32, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        if self.voices.is_empty() {
            return;
        }
        let sr = self.sr;
        let att = crate::params::env_seconds(params.attack.value()) * sr;
        let dec = crate::params::env_seconds(params.decay.value()) * sr;
        let rel = crate::params::env_seconds(params.release.value()) * sr;
        let sus = params.sustain.value();
        let gain = params.gain.value();
        let bright = params.bright.value();
        let prism = params.prism.value();
        let mix = params.mix.value();
        let channel_mode = params.part_mode.value() == MODE_CHANNEL;
        // Per-part weights for this block.
        let weights = [
            params.a.level.value() * if channel_mode { 1.0 } else { 1.0 - mix },
            params.b.level.value() * if channel_mode { 1.0 } else { mix },
        ];
        let parts_ref: [&PartParams; 2] = [&params.a, &params.b];
        // Per voice, per part tables (fundamental depends on the voice's note and the part's octave/detune).
        for voice in self.voices.iter_mut() {
            let base = 440.0 * 2f32.powf((voice.note as f32 - 69.0 + bend) / 12.0);
            let mut tables: [Option<PartTables>; 2] = [None, None];
            for p in 0..2 {
                if voice.parts[p] && weights[p] > 0.0 {
                    let part = parts_ref[p];
                    let f0 = base
                        * 2f32.powf(
                            (part.octave.value() as f32 * 12.0 + part.detune.value() + voice.uni_cents) / 12.0,
                        );
                    tables[p] = Some(part_tables(part, bright, prism, f0, sr));
                }
            }
            for i in 0..n {
                let env = voice.tick_env(att, dec, sus, rel);
                if env <= 0.0 {
                    continue;
                }
                let mut s = 0.0f32;
                for p in 0..2 {
                    let Some(t) = &tables[p] else { continue };
                    let part = parts_ref[p];
                    let f0 = base
                        * 2f32.powf(
                            (part.octave.value() as f32 * 12.0 + part.detune.value() + voice.uni_cents) / 12.0,
                        );
                    let mut ps = 0.0f32;
                    for k in 0..t.count {
                        let ph = &mut voice.phases[p][k];
                        *ph += f0 * t.mults[k] / sr;
                        if *ph >= 1.0 {
                            *ph -= (*ph).floor();
                        }
                        ps += t.amps[k] * sine(*ph);
                    }
                    s += ps * weights[p];
                }
                let out = s * env * voice.vel * gain * 0.6;
                l[i] += out;
                r[i] += out;
            }
        }
        self.voices.retain(|v| v.env != EnvPhase::Done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nih_plug::prelude::*;

    fn params() -> AdditiveParams {
        AdditiveParams::default()
    }

    fn render_block(e: &mut Engine, p: &AdditiveParams, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        e.render(p, 0.0, &mut l, &mut r);
        (l, r)
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn a_note_sounds_and_releases_to_silence() {
        let p = params();
        let mut e = Engine::new();
        e.note_on(&p, 69, 0, 1.0);
        let (l, _) = render_block(&mut e, &p, 4096);
        assert!(peak(&l) > 0.05, "expected audible output, peak {}", peak(&l));
        e.note_off(69, 0);
        // Render well past the release.
        let mut tail = 0.0f32;
        for _ in 0..40 {
            let (l, _) = render_block(&mut e, &p, 4096);
            tail = peak(&l);
        }
        assert!(e.active_voices() == 0, "voice should be gone after release");
        assert!(tail < 0.01, "tail should be silent, got {tail}");
    }

    #[test]
    fn channel_mode_routes_channel_2_to_part_b() {
        // This nih-plug fork exposes no public plain-value setter, so the tests swap in parameters whose
        // default is the wanted value.
        let mut p = params();
        // Part A: sine at octave 0. Part B: sine one octave up, on.
        p.a.shape = IntParam::new("Timbre shape", 3, IntRange::Linear { min: 0, max: 5 });
        p.b.shape = IntParam::new("Timbre shape", 3, IntRange::Linear { min: 0, max: 5 });
        p.b.octave = IntParam::new("Octave", 1, IntRange::Linear { min: -2, max: 2 });
        p.b.on = BoolParam::new("Part on", true);
        p.part_mode = IntParam::new("Part mode", MODE_CHANNEL, IntRange::Linear { min: 0, max: 3 });
        // Fundamental of a sine part ≈ zero crossings / 2 per second.
        let crossings = |note_ch: (u8, u8)| {
            let mut e = Engine::new();
            e.note_on(&p, note_ch.0, note_ch.1, 1.0);
            let (l, _) = render_block(&mut e, &p, 44100);
            l.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
        };
        let a = crossings((69, 0)); // 440 Hz
        let b = crossings((69, 1)); // 880 Hz via part B
        assert!((a as i32 - 440).abs() < 30, "part A crossings {a}");
        assert!((b as i32 - 880).abs() < 40, "part B crossings {b}");
    }

    #[test]
    fn no_parts_on_means_no_voice() {
        let mut p = params();
        p.a.on = BoolParam::new("Part on", false);
        let mut e = Engine::new();
        e.note_on(&p, 60, 0, 1.0);
        assert_eq!(e.active_voices(), 0);
    }
}
