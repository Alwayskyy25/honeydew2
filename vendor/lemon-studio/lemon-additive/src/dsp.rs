// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — original implementation (see params.rs note).

//! The additive engine, organised the way Harmor's panel is. Each voice sums up to [`MAX_PARTIALS`] sine
//! partials per part. A part's spectrum is built in the frequency domain: the two timbre shapes are blended
//! by the timbre mix, multiplied by the drawable harmonic spectrum, cloned by the harmonizer, then shaped by
//! the two spectral filters and the EQ — all as per-partial gains, the way Harmor manipulates partial data
//! rather than an audio stream. On top of that: prism stretches partial frequencies inharmonic, blur smooths
//! partial amplitudes over time, pluck decays high partials as the note ages, and a modulated allpass phaser
//! works on the summed voice. Voices carry vibrato, portamento / legato glide and strum delays.

use std::sync::OnceLock;

use crate::params::{
    AdditiveParams, FT_BANDPASS, FT_HIGHPASS, FT_LOWPASS, FT_NOTCH, FT_PEAK, MODE_A, MODE_B,
    MODE_CHANNEL, PARTIALS, PartParams, filter_hz,
};

/// Partials summed per part (Harmor uses up to 516; 64 keeps a phone CPU comfortable and covers most timbres).
pub const MAX_PARTIALS: usize = PARTIALS;
/// Voices including unison copies.
const MAX_VOICES: usize = 24;
const TABLE: usize = 2048;

fn sine_table() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        (0..=TABLE)
            .map(|i| (i as f32 / TABLE as f32 * std::f32::consts::TAU).sin())
            .collect()
    })
}

/// Sine of a phase in cycles (0..1), table + linear interpolation.
fn sine(phase: f32) -> f32 {
    let t = sine_table();
    let x = phase.fract() * TABLE as f32;
    let i = x as usize % TABLE;
    let f = x - x.floor();
    t[i] * (1.0 - f) + t[i + 1] * f
}

/// The harmonic recipe of a shape: the (signed) amplitude of partial `k` (1-based).
pub fn shape_amp(shape: i32, k: usize) -> f32 {
    let kf = k as f32;
    match shape {
        // Saw
        0 => 1.0 / kf,
        // Square
        1 => {
            if k % 2 == 1 {
                1.0 / kf
            } else {
                0.0
            }
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
            if k == 1 {
                1.0
            } else {
                0.0
            }
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

/// Prism: the frequency multiplier of partial `k` — prism stretches the harmonic series inharmonic,
/// pushing partials sharp as they rise (Harmor's metallic prism character).
pub fn prism_mult(k: usize, prism: f32) -> f32 {
    let kf = k as f32;
    kf.powf(1.0 + prism.clamp(0.0, 1.0) * 0.03)
}

/// Harmonizer: clones the spectrum `shift` partials up (shift from the width knob, 1..=12) with gain
/// `amount`, adding the clone to the original — Harmor's harmonizer thickens by transposed partial copies.
pub fn harmonize(amps: &[f32; MAX_PARTIALS], amount: f32, width: f32) -> [f32; MAX_PARTIALS] {
    if amount <= 0.0 {
        return *amps;
    }
    let shift = 1 + (width.clamp(0.0, 1.0) * 11.0).round() as usize;
    let mut out = *amps;
    for k in 0..MAX_PARTIALS {
        let src = k as isize - shift as isize;
        if src >= 0 {
            out[k] += amount * amps[src as usize];
        }
    }
    out
}

/// One spectral filter's gain at frequency `f`: `fc` the (already key-tracked / enveloped) cutoff, `res`
/// 0..1. Lowpass/highpass are 4th-order-ish slopes with a resonance bump at the corner; bandpass and peak
/// are log-Gaussian around fc (width narrows with resonance); notch cuts a log-Gaussian dip.
pub fn filter_gain(ftype: i32, f: f32, fc: f32, res: f32) -> f32 {
    if ftype == 0 || f <= 0.0 || fc <= 0.0 {
        return 1.0;
    }
    let ratio = (f / fc).max(1e-4);
    let bump = (-0.5 * (ratio.ln() / 0.22).powi(2)).exp(); // narrow Gaussian at the corner
    match ftype {
        FT_LOWPASS => {
            let base = 1.0 / (1.0 + ratio.powi(4));
            (base * (1.0 + res * 2.4 * bump)).min(1.6)
        }
        FT_HIGHPASS => {
            let base = 1.0 / (1.0 + ratio.powi(-4));
            (base * (1.0 + res * 2.4 * bump)).min(1.6)
        }
        FT_BANDPASS => {
            let w = 1.1 - res * 0.95;
            let g = (-0.5 * (ratio.ln() / w).powi(2)).exp();
            g * (0.35 + 0.65 * res.max(0.15))
        }
        FT_NOTCH => {
            let w = 0.55 - res * 0.42;
            let dip = (-0.5 * (ratio.ln() / w).powi(2)).exp();
            1.0 - (0.35 + 0.63 * res.max(0.2)) * dip
        }
        FT_PEAK => {
            let w = 0.8 - res * 0.6;
            let g = (-0.5 * (ratio.ln() / w).powi(2)).exp();
            1.0 + res * 1.6 * g
        }
        _ => 1.0,
    }
}

/// The 3-band EQ's gain at frequency `f`: a low shelf (below ~250 Hz), a mid bell (~1 kHz) and a high
/// shelf (above ~4 kHz); band knobs are -1..1 for ±12 dB.
pub fn eq_gain(low: f32, mid: f32, high: f32, f: f32) -> f32 {
    if f <= 0.0 {
        return 1.0;
    }
    let lf = f.log2();
    let shelf = |corner: f32, up: bool| {
        let x = (lf - corner.log2()) / 0.9;
        let t = (x * 0.5 + 0.5).clamp(0.0, 1.0);
        if up { t } else { 1.0 - t }
    };
    let mid_g = (-0.5 * ((lf - 1000.0f32.log2()) / 1.1).powi(2)).exp();
    let db =
        low * 12.0 * shelf(250.0, false) + mid * 12.0 * mid_g + high * 12.0 * shelf(4000.0, true);
    10f32.powf(db / 20.0)
}

/// Pluck: high partials die away as the note ages — the gain of partial `k` after `age` seconds.
pub fn pluck_factor(pluck: f32, k: usize, age: f32) -> f32 {
    if pluck <= 0.0 {
        return 1.0;
    }
    (-age * pluck * k as f32 * 0.22).exp()
}

/// One-pole approach of a whole partial bank toward `target` (the blur's time smoothing).
pub fn approach(cur: &mut [f32; MAX_PARTIALS], target: &[f32; MAX_PARTIALS], c: f32) {
    for i in 0..MAX_PARTIALS {
        cur[i] += (target[i] - cur[i]) * c;
    }
}

/// A modulated allpass phaser: 4 first-order allpass stages whose coefficient sweeps with an LFO.
/// `offset` sets the centre frequency, `width` the sweep depth, `speed` the LFO rate in Hz.
pub struct Phaser {
    x1: [f32; 4],
    y1: [f32; 4],
    phase: f32,
}

impl Default for Phaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Phaser {
    pub fn new() -> Self {
        Phaser {
            x1: [0.0; 4],
            y1: [0.0; 4],
            phase: 0.0,
        }
    }

    /// Processes one sample; `mix` blends dry/wet. The allpass coefficient follows
    /// a = (tan(pi f / sr) - 1) / (tan(pi f / sr) + 1) with f swept around the offset.
    pub fn process(
        &mut self,
        x: f32,
        sr: f32,
        offset_hz: f32,
        width: f32,
        speed_hz: f32,
        mix: f32,
    ) -> f32 {
        if mix <= 0.0 {
            return x;
        }
        self.phase += std::f32::consts::TAU * speed_hz / sr.max(1.0);
        if self.phase >= std::f32::consts::TAU {
            self.phase -= std::f32::consts::TAU;
        }
        let lfo = sine(self.phase / std::f32::consts::TAU);
        let f = (offset_hz * (1.0 + width.clamp(0.0, 1.0) * 0.9 * lfo)).clamp(30.0, sr * 0.42);
        let t = (std::f32::consts::PI * f / sr).tan();
        let a = (t - 1.0) / (t + 1.0);
        let mut s = x;
        for st in 0..4 {
            let y = a * s + self.x1[st] - a * self.y1[st];
            self.x1[st] = s;
            self.y1[st] = y;
            s = y;
        }
        x * (1.0 - mix) + s * mix
    }
}

/// One part's per-block tables: partial amplitudes (every spectral process applied) and frequency
/// multipliers (prism applied), plus the count of partials under Nyquist for fundamental `f0`.
struct PartTables {
    amps: [f32; MAX_PARTIALS],
    mults: [f32; MAX_PARTIALS],
    count: usize,
}

fn part_tables(part: &PartParams, spec: &[f32], env_level: f32, f0: f32, sr: f32) -> PartTables {
    let s1 = part.shape1.value();
    let s2 = part.shape2.value();
    let tm = part.tmix.value();
    let mut amps = [0.0f32; MAX_PARTIALS];
    let mut mults = [0.0f32; MAX_PARTIALS];
    let mut count = 0usize;
    for k in 1..=MAX_PARTIALS {
        let kf = k as f32;
        if f0 * kf >= sr * 0.45 {
            break;
        }
        count = k;
        mults[k - 1] = prism_mult(k, part.prism.value());
        let a = shape_amp(s1, k) * (1.0 - tm) + shape_amp(s2, k) * tm;
        amps[k - 1] = a * spec.get(k - 1).copied().unwrap_or(1.0);
    }
    amps = harmonize(&amps, part.harm.value(), part.harm_width.value());
    // Spectral filters + EQ, keyed on each partial's real frequency.
    let key_base = (f0 / 261.63).max(0.01);
    let fc = |freq_knob: f32, key: f32, env_amt: f32| {
        filter_hz(freq_knob) * key_base.powf(key) * 2f32.powf(env_amt * env_level * 4.0)
    };
    let (fc1, r1, t1) = (
        fc(
            part.f1_freq.value(),
            part.f1_key.value(),
            part.f1_env.value(),
        ),
        part.f1_res.value(),
        part.f1_type.value(),
    );
    let (fc2, r2, t2) = (
        fc(
            part.f2_freq.value(),
            part.f2_key.value(),
            part.f2_env.value(),
        ),
        part.f2_res.value(),
        part.f2_type.value(),
    );
    let (el, em, eh) = (
        part.eq_low.value(),
        part.eq_mid.value(),
        part.eq_high.value(),
    );
    let mut sum = 0.0f32;
    for k in 1..=count {
        let f = f0 * k as f32;
        let g = filter_gain(t1, f, fc1, r1) * filter_gain(t2, f, fc2, r2) * eq_gain(el, em, eh, f);
        amps[k - 1] *= g;
        sum += amps[k - 1].abs();
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
    /// Stereo pan -1..1 (unison spread).
    pan: f32,
    phases: [[f32; MAX_PARTIALS]; 2],
    /// Blur state: the smoothed partial banks, one per part.
    smooth: [[f32; MAX_PARTIALS]; 2],
    phaser: Phaser,
    env: EnvPhase,
    env_level: f32,
    /// Current (gliding) fundamental in Hz, without bend / vibrato.
    freq: f32,
    /// Target fundamental in Hz.
    target_freq: f32,
    lfo_phase: f32,
    age: u64,
    /// Samples left before the note starts (strum).
    delay: u32,
    age_samples: u64,
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

fn note_freq(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}

fn part_mult(part: &PartParams, uni_cents: f32) -> f32 {
    2f32.powf((part.octave.value() as f32 * 12.0 + part.detune.value() + uni_cents) / 12.0)
}

impl Engine {
    pub fn new() -> Self {
        Engine {
            sr: 44100.0,
            voices: Vec::new(),
            age: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        if sr > 0.0 {
            self.sr = sr;
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    fn parts_for(&self, params: &AdditiveParams, channel: u8) -> [bool; 2] {
        match params.part_mode.value() {
            MODE_A => [params.a.on.value(), false],
            MODE_B => [false, params.b.on.value()],
            MODE_CHANNEL => {
                // Piano roll colours in FL: channel 1 (green) = part A, channel 2 (red) = part B.
                if channel == 1 {
                    [false, true]
                } else {
                    [true, false]
                }
            }
            _ => [params.a.on.value(), params.b.on.value()],
        }
    }

    pub fn note_on(&mut self, params: &AdditiveParams, note: u8, channel: u8, velocity: f32) {
        // Legato: retarget the newest sounding voice instead of starting a new one.
        if params.legato.value() {
            let legato_parts = self.parts_for(params, channel);
            if let Some(v) = self
                .voices
                .iter_mut()
                .filter(|v| v.env != EnvPhase::Done)
                .max_by_key(|v| v.age)
            {
                v.note = note;
                v.channel = channel;
                v.parts = legato_parts;
                v.target_freq = note_freq(note);
                if v.env == EnvPhase::Release {
                    v.env = EnvPhase::Decay;
                }
                return;
            }
        }
        // A retriggered note releases its old voices first.
        self.note_off(note, channel);
        let parts = self.parts_for(params, channel);
        if !parts[0] && !parts[1] {
            return;
        }
        let u = params.unison.value().clamp(1, 4) as usize;
        let spread = params.udetune.value();
        let pan_spread = params.upan.value();
        let specs = [params.spectrum(0), params.spectrum(1)];
        let parts_ref: [&PartParams; 2] = [&params.a, &params.b];
        let f_note = note_freq(note);
        // With portamento, new voices glide from the last note's pitch.
        let start_freq = if params.porta.value() > 0.0 {
            self.voices.last().map_or(f_note, |v| v.target_freq)
        } else {
            f_note
        };
        for i in 0..u {
            if self.voices.len() >= MAX_VOICES {
                // Steal the oldest voice.
                if let Some((at, _)) = self.voices.iter().enumerate().min_by_key(|(_, v)| v.age) {
                    self.voices.remove(at);
                }
            }
            let frac = if u == 1 {
                0.0
            } else {
                i as f32 * 2.0 / (u - 1) as f32 - 1.0
            };
            let uni_cents = spread * frac;
            let delay = (params.strum.value() * 0.060 * self.sr * i as f32) as u32;
            // Seed the blur banks with the note's starting spectrum so blur only smooths later changes.
            let mut smooth = [[0.0f32; MAX_PARTIALS]; 2];
            for p in 0..2 {
                if parts[p] {
                    let f0 = f_note * part_mult(parts_ref[p], uni_cents);
                    smooth[p] = part_tables(parts_ref[p], &specs[p], 1.0, f0, self.sr).amps;
                }
            }
            self.age += 1;
            self.voices.push(Voice {
                note,
                channel,
                vel: velocity.clamp(0.0, 1.0),
                parts,
                uni_cents,
                pan: pan_spread * frac,
                phases: [[0.0; MAX_PARTIALS]; 2],
                smooth,
                phaser: Phaser::new(),
                env: EnvPhase::Attack,
                env_level: 0.0,
                freq: start_freq,
                target_freq: f_note,
                lfo_phase: 0.0,
                age: self.age,
                delay,
                age_samples: 0,
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
        let mix = params.mix.value();
        let channel_mode = params.part_mode.value() == MODE_CHANNEL;
        let weights = [
            params.a.level.value() * if channel_mode { 1.0 } else { 1.0 - mix },
            params.b.level.value() * if channel_mode { 1.0 } else { mix },
        ];
        let parts_ref: [&PartParams; 2] = [&params.a, &params.b];
        let specs = [params.spectrum(0), params.spectrum(1)];
        let vib_depth = params.vib_depth.value(); // cents
        let vib_hz = 0.5 + params.vib_speed.value() * 9.5;
        let porta_tau = params.porta.value() * params.porta.value() * 0.5;
        let glide_c = if porta_tau > 0.0005 {
            1.0 - (-1.0 / (porta_tau * sr)).exp()
        } else {
            1.0
        };
        let bend_mult = 2f32.powf(bend / 12.0);
        for voice in self.voices.iter_mut() {
            // Per-block tables and effective (blurred, plucked) banks per part.
            let mut tables: [Option<PartTables>; 2] = [None, None];
            let mut eff: [[f32; MAX_PARTIALS]; 2] = [[0.0; MAX_PARTIALS]; 2];
            for p in 0..2 {
                if voice.parts[p] && weights[p] > 0.0 {
                    let part = parts_ref[p];
                    let f0 = voice.freq * part_mult(part, voice.uni_cents);
                    let t = part_tables(part, &specs[p], voice.env_level, f0, sr);
                    // Blur: approach the target bank over the blur time, then mix raw/smoothed.
                    let bt = crate::params::blur_seconds(part.blur_time.value());
                    let c = 1.0 - (-(n as f32) / (bt * sr)).exp();
                    approach(&mut voice.smooth[p], &t.amps, c);
                    let bm = part.blur.value();
                    let age_s = voice.age_samples as f32 / sr;
                    for k in 0..t.count {
                        let a = t.amps[k] * (1.0 - bm) + voice.smooth[p][k] * bm;
                        eff[p][k] = a * pluck_factor(part.pluck.value(), k + 1, age_s);
                    }
                    tables[p] = Some(t);
                }
            }
            // Phaser settings come from the first sounding part.
            let ph_part = if tables[0].is_some() {
                parts_ref[0]
            } else {
                parts_ref[1]
            };
            let (ph_mix, ph_w, ph_o, ph_s) = (
                ph_part.phaser_mix.value(),
                ph_part.phaser_width.value(),
                crate::params::filter_hz(ph_part.phaser_ofs.value()),
                0.05 + ph_part.phaser_speed.value() * ph_part.phaser_speed.value() * 8.0,
            );
            for i in 0..n {
                if voice.delay > 0 {
                    voice.delay -= 1;
                    continue;
                }
                let env = voice.tick_env(att, dec, sus, rel);
                if env <= 0.0 {
                    continue;
                }
                voice.age_samples += 1;
                // Glide + vibrato.
                voice.freq += (voice.target_freq - voice.freq) * glide_c;
                voice.lfo_phase += std::f32::consts::TAU * vib_hz / sr;
                if voice.lfo_phase >= std::f32::consts::TAU {
                    voice.lfo_phase -= std::f32::consts::TAU;
                }
                let vib =
                    2f32.powf(vib_depth * sine(voice.lfo_phase / std::f32::consts::TAU) / 1200.0);
                let fbase = voice.freq * bend_mult * vib;
                let mut s = 0.0f32;
                for p in 0..2 {
                    let Some(t) = &tables[p] else { continue };
                    let f0 = fbase * part_mult(parts_ref[p], voice.uni_cents);
                    let mut ps = 0.0f32;
                    for k in 0..t.count {
                        let ph = &mut voice.phases[p][k];
                        *ph += f0 * t.mults[k] / sr;
                        if *ph >= 1.0 {
                            *ph -= (*ph).floor();
                        }
                        ps += eff[p][k] * sine(*ph);
                    }
                    s += ps * weights[p];
                }
                s = voice.phaser.process(s, sr, ph_o, ph_w, ph_s, ph_mix);
                let out = s * env * voice.vel * gain * 0.6;
                // Equal-power pan.
                let ang = (voice.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
                l[i] += out * ang.cos();
                r[i] += out * ang.sin();
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
        assert!(
            peak(&l) > 0.05,
            "expected audible output, peak {}",
            peak(&l)
        );
        e.note_off(69, 0);
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
        p.a.shape1 = IntParam::new("Timbre 1", 3, IntRange::Linear { min: 0, max: 5 });
        p.a.shape2 = IntParam::new("Timbre 2", 3, IntRange::Linear { min: 0, max: 5 });
        p.a.tmix = FloatParam::new("Timbre mix", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 });
        p.b.shape1 = IntParam::new("Timbre 1", 3, IntRange::Linear { min: 0, max: 5 });
        p.b.shape2 = IntParam::new("Timbre 2", 3, IntRange::Linear { min: 0, max: 5 });
        p.b.octave = IntParam::new("Octave", 1, IntRange::Linear { min: -2, max: 2 });
        p.b.on = BoolParam::new("Part on", true);
        p.part_mode = IntParam::new(
            "Part mode",
            MODE_CHANNEL,
            IntRange::Linear { min: 0, max: 3 },
        );
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

    #[test]
    fn prism_stretches_partials_inharmonic() {
        assert_eq!(prism_mult(1, 1.0), 1.0);
        assert!((prism_mult(10, 0.0) - 10.0).abs() < 1e-4);
        // At full prism the 10th partial sits well sharp of 10x.
        assert!(prism_mult(10, 1.0) > 10.5, "got {}", prism_mult(10, 1.0));
        // Stretch grows with partial index.
        assert!(prism_mult(32, 1.0) / 32.0 > prism_mult(10, 1.0) / 10.0);
    }

    #[test]
    fn harmonizer_clones_the_spectrum_up() {
        let mut amps = [0.0f32; MAX_PARTIALS];
        amps[0] = 1.0;
        let out = harmonize(&amps, 0.5, 0.0); // width 0 -> shift 1
        assert_eq!(out[0], 1.0);
        assert!(
            (out[1] - 0.5).abs() < 1e-6,
            "clone at partial 2, got {}",
            out[1]
        );
        assert_eq!(out[2], 0.0);
        let wide = harmonize(&amps, 1.0, 1.0); // width 1 -> shift 12
        assert!((wide[12] - 1.0).abs() < 1e-6);
        let off = harmonize(&amps, 0.0, 0.5);
        assert_eq!(off, amps);
    }

    #[test]
    fn pluck_decays_high_partials_first() {
        assert_eq!(pluck_factor(0.0, 32, 5.0), 1.0);
        assert_eq!(pluck_factor(1.0, 8, 0.0), 1.0);
        let lo = pluck_factor(1.0, 2, 1.0);
        let hi = pluck_factor(1.0, 32, 1.0);
        assert!(hi < lo && lo < 1.0, "lo {lo} hi {hi}");
        assert!(pluck_factor(1.0, 32, 2.0) < hi, "decay continues with age");
    }

    #[test]
    fn lowpass_filter_rolls_off_and_resonance_bumps() {
        assert!(filter_gain(FT_LOWPASS, 100.0, 1000.0, 0.0) > 0.95);
        assert!(filter_gain(FT_LOWPASS, 8000.0, 1000.0, 0.0) < 0.05);
        let plain = filter_gain(FT_LOWPASS, 1000.0, 1000.0, 0.0);
        let res = filter_gain(FT_LOWPASS, 1000.0, 1000.0, 1.0);
        assert!(res > plain * 1.5, "res {res} vs plain {plain}");
        assert!(filter_gain(FT_HIGHPASS, 100.0, 1000.0, 0.0) < 0.05);
        assert!(filter_gain(FT_HIGHPASS, 8000.0, 1000.0, 0.0) > 0.95);
        assert!(
            filter_gain(FT_BANDPASS, 1000.0, 1000.0, 0.5)
                > filter_gain(FT_BANDPASS, 4000.0, 1000.0, 0.5)
        );
        assert!(filter_gain(FT_NOTCH, 1000.0, 1000.0, 1.0) < 0.4);
        assert!(filter_gain(FT_PEAK, 1000.0, 1000.0, 1.0) > 1.5);
        assert_eq!(filter_gain(0, 500.0, 1000.0, 1.0), 1.0);
    }

    #[test]
    fn eq_shelves_and_bell() {
        assert!(eq_gain(1.0, 0.0, 0.0, 60.0) > 2.0, "low shelf boosts lows");
        assert!((eq_gain(1.0, 0.0, 0.0, 12000.0) - 1.0).abs() < 0.15);
        assert!(
            eq_gain(0.0, 0.0, 1.0, 12000.0) > 2.0,
            "high shelf boosts highs"
        );
        assert!(eq_gain(0.0, 1.0, 0.0, 1000.0) > 2.0, "mid bell at 1 kHz");
        assert!(
            (eq_gain(0.0, 0.0, 0.0, 440.0) - 1.0).abs() < 1e-6,
            "flat EQ is unity"
        );
    }

    #[test]
    fn phaser_is_bounded_and_moves() {
        let mut ph = Phaser::new();
        assert_eq!(
            ph.process(0.7, 44100.0, 800.0, 0.8, 2.0, 0.0),
            0.7,
            "mix 0 is dry"
        );
        let mut outs = Vec::new();
        for i in 0..4410 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            outs.push(ph.process(x, 44100.0, 800.0, 0.8, 2.0, 1.0));
        }
        assert!(
            outs.iter().all(|v| v.is_finite() && v.abs() <= 1.0),
            "allpass chain stays bounded"
        );
        assert!(
            outs.iter().take(2000).any(|v| v.abs() > 0.01),
            "wet path carries signal"
        );
    }

    #[test]
    fn blur_approach_converges() {
        let mut cur = [0.0f32; MAX_PARTIALS];
        let target = [1.0f32; MAX_PARTIALS];
        approach(&mut cur, &target, 0.5);
        assert!((cur[0] - 0.5).abs() < 1e-6);
        for _ in 0..40 {
            approach(&mut cur, &target, 0.5);
        }
        assert!((cur[0] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn drawable_spectrum_shapes_the_sound() {
        let mut p = params();
        p.a.shape1 = IntParam::new("Timbre 1", 3, IntRange::Linear { min: 0, max: 5 }); // sine recipe
        p.a.tmix = FloatParam::new("Timbre mix", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 });
        // Kill every partial in the drawn spectrum: silence.
        p.set_spectrum_all(0, &[0.0; PARTIALS]);
        let mut e = Engine::new();
        e.note_on(&p, 69, 0, 1.0);
        let (l, _) = render_block(&mut e, &p, 4096);
        assert!(
            peak(&l) < 0.01,
            "empty spectrum should be silent, peak {}",
            peak(&l)
        );
        // Restore only the fundamental: a clean sine at the note frequency.
        let mut spec = [0.0f32; PARTIALS];
        spec[0] = 1.0;
        p.set_spectrum_all(0, &spec);
        let mut e = Engine::new();
        e.note_on(&p, 69, 0, 1.0);
        let (l, _) = render_block(&mut e, &p, 44100);
        let crossings = l.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((crossings as i32 - 440).abs() < 30, "crossings {crossings}");
        assert!(peak(&l) > 0.1);
    }

    #[test]
    fn legato_retargets_the_sounding_voice() {
        let mut p = params();
        p.legato = BoolParam::new("Legato", true);
        let mut e = Engine::new();
        e.note_on(&p, 69, 0, 1.0);
        assert_eq!(e.active_voices(), 1);
        e.note_on(&p, 76, 0, 1.0);
        assert_eq!(e.active_voices(), 1, "legato must not stack voices");
        assert_eq!(e.voices[0].note, 76);
    }

    #[test]
    fn strum_delays_later_unison_voices() {
        let mut p = params();
        p.unison = IntParam::new("Unison voices", 3, IntRange::Linear { min: 1, max: 4 });
        p.strum = FloatParam::new("Strum", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 });
        let mut e = Engine::new();
        e.note_on(&p, 69, 0, 1.0);
        assert_eq!(e.active_voices(), 3);
        assert_eq!(e.voices[0].delay, 0);
        assert!(
            e.voices[1].delay > 2000 && e.voices[2].delay > e.voices[1].delay,
            "strum staggers starts"
        );
    }
}
