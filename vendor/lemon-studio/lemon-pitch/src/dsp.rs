// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Pitch — original implementation (see params.rs note).

//! Analysis and playback. On load, the sample's pitch is tracked with the YIN method (`os_dsp::pitch`) on a
//! 512-frame hop; contiguous frames of like pitch become segments (Newtone's orange blocks), each with a target
//! note the editor can move. Playback runs the sample through the MPCS stretch stream (`os_dsp::stretch`) whose
//! transpose follows the correction curve: duration is preserved, pitch is pulled toward each segment's target
//! by the Center amount, keeping as much of the original contour as Variation says, smoothed over Transition.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use os_dsp::stretch::Stream;
use os_engine::Sample;

use crate::params::PitchParams;

/// Frames between pitch frames.
pub const HOP: usize = 512;
/// Frames of audio a pitch frame is measured over.
const WINDOW: usize = 2048;

/// One detected note segment: hop frames `start..end`, the median detected pitch and the target note
/// (a whole note unless the user dragged the block).
#[derive(Clone, Debug)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub median: f32,
    pub target: f32,
}

/// A loaded sample's pitch analysis.
#[derive(Clone)]
pub struct Analysis {
    /// Detected pitch per hop frame as a float MIDI note; `None` = unvoiced / quiet.
    pub track: Vec<Option<f32>>,
    /// The segments, in time order.
    pub segments: Vec<Segment>,
    /// Segment index per hop frame.
    pub seg_of: Vec<Option<usize>>,
}

impl Analysis {
    /// Analyses a sample (mono mixdown) — run on a load, never on the audio thread.
    pub fn of(sample: &Sample) -> Analysis {
        let n = sample.frames();
        let mono: Vec<f32> = (0..n).map(|i| 0.5 * (sample.left[i] + sample.right[i])).collect();
        let mut track: Vec<Option<f32>> = Vec::new();
        let mut at = 0usize;
        while at + WINDOW <= n {
            let hz = os_dsp::pitch::yin(&mono[at..at + WINDOW], sample.rate);
            track.push(hz.map(|h| 69.0 + 12.0 * (h / 440.0).log2() as f32));
            at += HOP;
        }
        // Segments: runs of voiced frames whose pitch stays within 0.6 semitones of the run's running median;
        // runs shorter than 4 frames (~25 ms) fold into their neighbours as unvoiced.
        let mut segments: Vec<Segment> = Vec::new();
        let mut seg_of: Vec<Option<usize>> = vec![None; track.len()];
        let mut i = 0usize;
        while i < track.len() {
            let Some(first) = track[i] else {
                i += 1;
                continue;
            };
            let mut vals = vec![first];
            let mut j = i + 1;
            while j < track.len() {
                let Some(v) = track[j] else { break };
                let mut sorted = vals.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let med = sorted[sorted.len() / 2];
                if (v - med).abs() > 0.6 {
                    break;
                }
                vals.push(v);
                j += 1;
            }
            if j - i >= 4 {
                let mut sorted = vals.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let median = sorted[sorted.len() / 2];
                let idx = segments.len();
                segments.push(Segment { start: i, end: j, median, target: median.round() });
                for s in seg_of.iter_mut().take(j).skip(i) {
                    *s = Some(idx);
                }
            }
            i = j.max(i + 1);
        }
        Analysis { track, segments, seg_of }
    }

    /// The correction (semitones to shift) at source frame `frame`, before Transition smoothing.
    pub fn shift_at(&self, frame: usize, center: f32, variation: f32) -> f32 {
        let i = frame / HOP;
        let Some(Some(s)) = self.seg_of.get(i) else { return 0.0 };
        let seg = &self.segments[*s];
        let Some(detected) = self.track.get(i).copied().flatten() else { return 0.0 };
        let contour = (detected - seg.median) * variation;
        (seg.target + contour - detected) * center
    }
}

/// State the editor and the audio side share: the loaded sample, its analysis and its path.
pub struct Shared {
    sample: RwLock<Option<Arc<Sample>>>,
    analysis: RwLock<Option<Arc<Analysis>>>,
    /// Segment targets, edited in the editor; mirrored into the analysis segments on change.
    targets: Mutex<Vec<f32>>,
    path: RwLock<String>,
    version: AtomicU32,
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

impl Shared {
    pub fn new() -> Self {
        Shared {
            sample: RwLock::new(None),
            analysis: RwLock::new(None),
            targets: Mutex::new(Vec::new()),
            path: RwLock::new(String::new()),
            version: AtomicU32::new(0),
        }
    }

    pub fn version(&self) -> u32 {
        self.version.load(Ordering::Acquire)
    }

    pub fn loaded_path(&self) -> String {
        self.path.read().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn sample(&self) -> Option<Arc<Sample>> {
        self.sample.read().ok().and_then(|s| s.clone())
    }

    pub fn analysis(&self) -> Option<Arc<Analysis>> {
        self.analysis.read().ok().and_then(|a| a.clone())
    }

    /// Loads and analyses `path`. On success the shared state is swapped and the version bumped.
    pub fn load(&self, path: &str) -> Result<(), String> {
        let sample = os_engine::sample::load_shared(Path::new(path)).map_err(|e| format!("{e:?}"))?;
        let mut analysis = Analysis::of(&sample);
        // Fresh targets from the analysis.
        let targets: Vec<f32> = analysis.segments.iter().map(|s| s.target).collect();
        for (seg, t) in analysis.segments.iter_mut().zip(targets.iter()) {
            seg.target = *t;
        }
        if let Ok(mut s) = self.sample.write() {
            *s = Some(sample);
        }
        if let Ok(mut a) = self.analysis.write() {
            *a = Some(Arc::new(analysis));
        }
        if let Ok(mut t) = self.targets.lock() {
            *t = targets;
        }
        if let Ok(mut p) = self.path.write() {
            *p = path.to_string();
        }
        self.version.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Moves segment `seg`'s target note (the editor's block drag); the analysis is updated in place.
    pub fn set_target(&self, seg: usize, note: f32) {
        if let Ok(mut t) = self.targets.lock() {
            if let Some(v) = t.get_mut(seg) {
                *v = note;
            }
        }
        if let Ok(mut a) = self.analysis.write() {
            if let Some(arc) = a.as_mut() {
                let inner = Arc::make_mut(arc);
                if let Some(s) = inner.segments.get_mut(seg) {
                    s.target = note;
                }
            }
        }
        self.version.fetch_add(1, Ordering::AcqRel);
    }
}

/// One playing voice: the sample through the stretch stream, transpose following the correction curve.
struct Voice {
    note: u8,
    channel: u8,
    vel: f32,
    st: Stream,
    input: Vec<f32>,
    rpos: i64,
    owed: f64,
    skip: usize,
    l: Vec<f32>,
    r: Vec<f32>,
    at: usize,
    /// Output frames played (the source position is this many frames in at speed 1).
    played: usize,
    /// Smoothed correction in semitones (the Transition one-pole).
    shift: f32,
    /// Release fade: `Some(frames left)` after note-off.
    release: Option<usize>,
    gain_now: f32,
}

const BLOCK: usize = 32;

impl Voice {
    fn start(sample: &Sample, note: u8, channel: u8, vel: f32) -> Voice {
        let mut st = Stream::new(sample.rate);
        let lead = st.lead();
        let mut input = Vec::with_capacity(2 * (lead + BLOCK * 2));
        for i in 0..lead {
            let (a, b) = if i < sample.frames() { (sample.left[i], sample.right[i]) } else { (0.0, 0.0) };
            input.push(a);
            input.push(b);
        }
        st.set_transpose(1.0);
        st.start(&input, 1.0);
        let skip = st.delay();
        Voice {
            note,
            channel,
            vel,
            st,
            input: Vec::new(),
            rpos: lead as i64,
            owed: 0.0,
            skip,
            l: vec![0.0; BLOCK],
            r: vec![0.0; BLOCK],
            at: BLOCK,
            played: 0,
            shift: 0.0,
            release: None,
            gain_now: 0.0,
        }
    }

    fn feed(&mut self, s: &Sample) {
        let (a, b) = match usize::try_from(self.rpos) {
            Ok(i) if i < s.frames() => (s.left[i], s.right[i]),
            _ => (0.0, 0.0),
        };
        self.input.push(a);
        self.input.push(b);
        self.rpos += 1;
    }
}

/// The playback engine.
pub struct Engine {
    shared: Arc<Shared>,
    voices: Vec<Voice>,
    sr: f32,
}

impl Engine {
    pub fn new(shared: Arc<Shared>) -> Self {
        Engine { shared, voices: Vec::new(), sr: 44100.0 }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        if sr > 0.0 {
            self.sr = sr;
        }
    }

    pub fn note_on(&mut self, note: u8, channel: u8, velocity: f32) {
        let Some(sample) = self.shared.sample() else { return };
        self.note_off(note, channel);
        if self.voices.len() >= 8 {
            self.voices.remove(0);
        }
        self.voices.push(Voice::start(&sample, note, channel, velocity.clamp(0.0, 1.0)));
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        for v in self.voices.iter_mut() {
            if v.note == note && v.channel == channel && v.release.is_none() {
                v.release = Some(1323); // ~30 ms at 44.1 kHz
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        self.voices.clear();
    }

    /// Renders (adds into) stereo buffers. The played note transposes the whole line against the sample's
    /// root (note 60 = the sample as recorded); the correction curve applies on top.
    pub fn render(&mut self, params: &PitchParams, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let (Some(sample), Some(analysis)) = (self.shared.sample(), self.shared.analysis()) else {
            return;
        };
        let center = params.center.value();
        let variation = params.variation.value();
        let transition = params.transition.value();
        let gain = params.gain.value();
        // One-pole coefficient for the Transition smoothing (5 ms .. ~250 ms).
        let tau = 0.005 + transition * transition * 0.245;
        let k = 1.0 - (-1.0 / (tau * self.sr)).exp();
        for voice in self.voices.iter_mut() {
            let base = (voice.note as f32 - 60.0) / 12.0;
            for i in 0..n {
                // Refill the stretch block.
                if voice.at >= BLOCK {
                    let raw = analysis.shift_at(voice.played, center, variation);
                    voice.shift += (raw - voice.shift) * k;
                    voice.owed += BLOCK as f64;
                    let take = voice.owed.floor();
                    voice.owed -= take;
                    voice.input.clear();
                    for _ in 0..take as usize {
                        voice.feed(&sample);
                    }
                    // The stream's transpose is a pitch factor: the note's octave offset plus the correction.
                    voice.st.set_transpose(2f64.powf(base as f64 + voice.shift as f64 / 12.0));
                    voice.st.process(&voice.input, &mut voice.l, &mut voice.r);
                    voice.at = 0;
                }
                let (mut a, mut b) = (voice.l[voice.at], voice.r[voice.at]);
                voice.at += 1;
                voice.played += 1;
                if voice.skip > 0 {
                    voice.skip -= 1;
                    a = 0.0;
                    b = 0.0;
                }
                // Short attack fade and the release fade.
                let target = if voice.release.is_some() { 0.0 } else { 1.0 };
                let step = if target > voice.gain_now { 1.0 / 128.0 } else { 1.0 / 1323.0 };
                if voice.gain_now < target {
                    voice.gain_now = (voice.gain_now + step).min(target);
                } else if voice.gain_now > target {
                    voice.gain_now = (voice.gain_now - step).max(target);
                }
                if let Some(left) = &mut voice.release {
                    *left = left.saturating_sub(1);
                }
                let g = voice.gain_now * voice.vel * gain;
                l[i] += a * g;
                r[i] += b * g;
            }
        }
        self.voices.retain(|v| v.release != Some(0) && v.played < sample.frames() + 44100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, seconds: f32, rate: u32) -> Sample {
        let n = (seconds * rate as f32) as usize;
        let left: Vec<f32> = (0..n).map(|i| (i as f32 / rate as f32 * freq * std::f32::consts::TAU).sin() * 0.5).collect();
        Sample { rate, right: left.clone(), left }
    }

    #[test]
    fn analysis_finds_the_tone_and_its_segment() {
        // A slightly flat A4 (430 Hz) for half a second.
        let s = tone(430.0, 0.5, 44100);
        let a = Analysis::of(&s);
        assert!(!a.segments.is_empty(), "expected a segment");
        let seg = &a.segments[0];
        assert!((seg.median - 68.6).abs() < 0.4, "median {} should be ~68.6 (430 Hz)", seg.median);
        assert_eq!(seg.target, 69.0, "target snaps to A4");
        // Full center pulls the flat tone up by ~+0.4 semitones.
        let shift = a.shift_at(HOP * 6, 1.0, 0.0);
        assert!((shift - 0.4).abs() < 0.25, "shift {shift}");
        // No center, no shift.
        assert!(a.shift_at(HOP * 6, 0.0, 0.0).abs() < 1e-6);
    }

    #[test]
    fn two_tones_make_two_segments() {
        let rate = 44100u32;
        let mut left = Vec::new();
        for (f, secs) in [(440.0f32, 0.4f32), (523.25, 0.4)] {
            let n = (secs * rate as f32) as usize;
            left.extend((0..n).map(|i| (i as f32 / rate as f32 * f * std::f32::consts::TAU).sin() * 0.5));
        }
        let s = Sample { rate, right: left.clone(), left };
        let a = Analysis::of(&s);
        assert!(a.segments.len() >= 2, "expected >= 2 segments, got {}", a.segments.len());
        assert!((a.segments[0].median - 69.0).abs() < 0.3);
        assert!((a.segments[1].median - 72.0).abs() < 0.3);
    }
}
