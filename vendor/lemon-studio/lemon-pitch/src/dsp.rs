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
/// (a whole note unless the user dragged the block). `lo`/`hi` are the lowest / highest detected pitches
/// inside the segment — Newtone draws the note block spanning that range, so its height shows how
/// unstable the pitch was.
#[derive(Clone, Debug)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub median: f32,
    pub target: f32,
    pub lo: f32,
    pub hi: f32,
}

/// Snaps a note to the nearest note of a scale (Newtone's "Snap to scale"): scale 0 is chromatic
/// (plain semitone rounding), 1 major, 2 minor, with `root` the scale's tonic pitch class (0 = C).
pub fn snap_note(note: f32, scale: i32, root: i32) -> f32 {
    let r = note.round() as i32;
    if scale <= 0 {
        return r as f32;
    }
    let iv: &[i32] = if scale == 1 {
        &[0, 2, 4, 5, 7, 9, 11]
    } else {
        &[0, 2, 3, 5, 7, 8, 10]
    };
    let mut best = r;
    let mut bd = f32::MAX;
    for cand in (r - 2)..=(r + 2) {
        if iv.contains(&(cand - root).rem_euclid(12)) {
            let d = (cand as f32 - note).abs();
            if d < bd {
                bd = d;
                best = cand;
            }
        }
    }
    best as f32
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
        let mono: Vec<f32> = (0..n)
            .map(|i| 0.5 * (sample.left[i] + sample.right[i]))
            .collect();
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
                let lo = *sorted.first().unwrap_or(&median);
                let hi = *sorted.last().unwrap_or(&median);
                let idx = segments.len();
                segments.push(Segment {
                    start: i,
                    end: j,
                    median,
                    target: median.round(),
                    lo,
                    hi,
                });
                for s in seg_of.iter_mut().take(j).skip(i) {
                    *s = Some(idx);
                }
            }
            i = j.max(i + 1);
        }
        Analysis {
            track,
            segments,
            seg_of,
        }
    }

    /// The correction (semitones to shift) at source frame `frame`, before Transition smoothing.
    /// `snap` = Some((scale, root)) makes the segment's target land on the scale first (snap-to-scale).
    pub fn shift_at(
        &self,
        frame: usize,
        center: f32,
        variation: f32,
        snap: Option<(i32, i32)>,
    ) -> f32 {
        let i = frame / HOP;
        let Some(Some(s)) = self.seg_of.get(i) else {
            return 0.0;
        };
        let seg = &self.segments[*s];
        let Some(detected) = self.track.get(i).copied().flatten() else {
            return 0.0;
        };
        let target = match snap {
            Some((scale, root)) => snap_note(seg.target, scale, root),
            None => seg.target,
        };
        let contour = (detected - seg.median) * variation;
        (target + contour - detected) * center
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
        let sample =
            os_engine::sample::load_shared(Path::new(path)).map_err(|e| format!("{e:?}"))?;
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
            let (a, b) = if i < sample.frames() {
                (sample.left[i], sample.right[i])
            } else {
                (0.0, 0.0)
            };
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
        Engine {
            shared,
            voices: Vec::new(),
            sr: 44100.0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        if sr > 0.0 {
            self.sr = sr;
        }
    }

    pub fn note_on(&mut self, note: u8, channel: u8, velocity: f32) {
        let Some(sample) = self.shared.sample() else {
            return;
        };
        self.note_off(note, channel);
        if self.voices.len() >= 8 {
            self.voices.remove(0);
        }
        self.voices.push(Voice::start(
            &sample,
            note,
            channel,
            velocity.clamp(0.0, 1.0),
        ));
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
        let snap = if params.snap.value() {
            Some((params.scale.value(), params.root.value()))
        } else {
            None
        };
        let gain = params.gain.value();
        // One-pole coefficient for the Transition smoothing (5 ms .. ~250 ms).
        let tau = 0.005 + transition * transition * 0.245;
        let k = 1.0 - (-1.0 / (tau * self.sr)).exp();
        for voice in self.voices.iter_mut() {
            let base = (voice.note as f32 - 60.0) / 12.0;
            for i in 0..n {
                // Refill the stretch block.
                if voice.at >= BLOCK {
                    let raw = analysis.shift_at(voice.played, center, variation, snap);
                    voice.shift += (raw - voice.shift) * k;
                    voice.owed += BLOCK as f64;
                    let take = voice.owed.floor();
                    voice.owed -= take;
                    voice.input.clear();
                    for _ in 0..take as usize {
                        voice.feed(&sample);
                    }
                    // The stream's transpose is a pitch factor: the note's octave offset plus the correction.
                    voice
                        .st
                        .set_transpose(2f64.powf(base as f64 + voice.shift as f64 / 12.0));
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
                let step = if target > voice.gain_now {
                    1.0 / 128.0
                } else {
                    1.0 / 1323.0
                };
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
        self.voices
            .retain(|v| v.release != Some(0) && v.played < sample.frames() + 44100);
    }
}

/// Renders the whole loaded sample through the correction path at the root note (60 = as recorded):
/// the same voices, correction curve and Transition smoothing playback uses, run offline. The result
/// has exactly the source's frame count (the stretch stream preserves duration).
pub fn render_corrected(shared: &Arc<Shared>, params: &PitchParams) -> Option<Sample> {
    let src = shared.sample()?;
    let total = src.frames();
    let mut engine = Engine::new(shared.clone());
    engine.set_sample_rate(src.rate as f32);
    engine.note_on(60, 0, 1.0);
    let mut l: Vec<f32> = Vec::with_capacity(total);
    let mut r: Vec<f32> = Vec::with_capacity(total);
    while l.len() < total {
        let n = 1024.min(total - l.len());
        let mut lb = vec![0.0f32; n];
        let mut rb = vec![0.0f32; n];
        engine.render(params, &mut lb, &mut rb);
        l.extend_from_slice(&lb);
        r.extend_from_slice(&rb);
        if engine.voices.is_empty() {
            break;
        }
    }
    l.resize(total, 0.0);
    r.resize(total, 0.0);
    Some(Sample {
        rate: src.rate,
        left: l,
        right: r,
    })
}

/// A minimal 16-bit PCM stereo WAV encoding of a sample.
pub fn write_wav(s: &Sample) -> Vec<u8> {
    let frames = s.frames();
    let data_len = frames * 4;
    let mut out = Vec::with_capacity(44 + data_len);
    let mut push = |b: &[u8]| out.extend_from_slice(b);
    push(b"RIFF");
    push(&(36 + data_len as u32).to_le_bytes());
    push(b"WAVE");
    push(b"fmt ");
    push(&16u32.to_le_bytes());
    push(&1u16.to_le_bytes()); // PCM
    push(&2u16.to_le_bytes()); // stereo
    push(&s.rate.to_le_bytes());
    push(&(s.rate * 4).to_le_bytes());
    push(&4u16.to_le_bytes());
    push(&16u16.to_le_bytes());
    push(b"data");
    push(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        for v in [s.left[i], s.right[i]] {
            push(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    out
}

/// Renders the corrected audio and saves it through `os_plugkit::save_file` — on Honeydew the host
/// keeps it in the user library (My Recordings) under the returned name, where the sample picker and
/// every other plugin can load it; elsewhere it is written under that name. This is the drag-out
/// equivalent: Newtone's edited audio, usable anywhere.
pub fn export_corrected(shared: &Arc<Shared>, params: &PitchParams) -> Result<String, String> {
    let out = render_corrected(shared, params).ok_or_else(|| "No sample loaded".to_string())?;
    let path = shared.loaded_path();
    let stem = path
        .rsplit(['/', '\\'])
        .next()
        .and_then(|n| n.rsplit_once('.').map(|(s, _)| s).or(Some(n)))
        .filter(|s| !s.is_empty())
        .unwrap_or("Lemon Pitch");
    let name = format!("{stem} (corrected).wav");
    os_plugkit::save_file(Path::new(&name), write_wav(&out)).map_err(|e| e.to_string())?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, seconds: f32, rate: u32) -> Sample {
        let n = (seconds * rate as f32) as usize;
        let left: Vec<f32> = (0..n)
            .map(|i| (i as f32 / rate as f32 * freq * std::f32::consts::TAU).sin() * 0.5)
            .collect();
        Sample {
            rate,
            right: left.clone(),
            left,
        }
    }

    #[test]
    fn analysis_finds_the_tone_and_its_segment() {
        // A slightly flat A4 (430 Hz) for half a second.
        let s = tone(430.0, 0.5, 44100);
        let a = Analysis::of(&s);
        assert!(!a.segments.is_empty(), "expected a segment");
        let seg = &a.segments[0];
        assert!(
            (seg.median - 68.6).abs() < 0.4,
            "median {} should be ~68.6 (430 Hz)",
            seg.median
        );
        assert_eq!(seg.target, 69.0, "target snaps to A4");
        // Full center pulls the flat tone up by ~+0.4 semitones.
        let shift = a.shift_at(HOP * 6, 1.0, 0.0, None);
        assert!((shift - 0.4).abs() < 0.25, "shift {shift}");
        // No center, no shift.
        assert!(a.shift_at(HOP * 6, 0.0, 0.0, None).abs() < 1e-6);
        // The segment's pitch range brackets its median.
        assert!(
            seg.lo <= seg.median && seg.median <= seg.hi,
            "lo {} med {} hi {}",
            seg.lo,
            seg.median,
            seg.hi
        );
    }

    #[test]
    fn snap_note_lands_on_the_scale() {
        // Chromatic = plain rounding.
        assert_eq!(snap_note(69.4, 0, 0), 69.0);
        assert_eq!(snap_note(69.6, 0, 0), 70.0);
        // C major: F# (66) is out of scale; nearest scale notes are F (65) and G (67).
        assert_eq!(snap_note(66.0, 1, 0), 65.0);
        assert_eq!(snap_note(66.4, 1, 0), 67.0);
        // A minor (root A=9): C (72) and B (71) are in scale, C# (73) is not.
        assert_eq!(snap_note(72.0, 2, 9), 72.0);
        assert_eq!(snap_note(73.0, 2, 9), 72.0);
        // D major (root D=2): F# (66) IS in scale.
        assert_eq!(snap_note(66.0, 1, 2), 66.0);
    }

    #[test]
    fn shift_at_applies_snap_to_the_target() {
        // One segment, target dragged to F#4 (66) which C major forbids.
        let mut a = Analysis {
            track: vec![Some(66.0); 8],
            segments: Vec::new(),
            seg_of: vec![Some(0); 8],
        };
        a.segments.push(Segment {
            start: 0,
            end: 8,
            median: 66.0,
            target: 66.0,
            lo: 66.0,
            hi: 66.0,
        });
        // Unsnapped: detected == target, no shift. Snapped to C major: target becomes F4 (65), shift -1.
        assert!(a.shift_at(HOP * 2, 1.0, 0.0, None).abs() < 1e-6);
        let snapped = a.shift_at(HOP * 2, 1.0, 0.0, Some((1, 0)));
        assert!((snapped + 1.0).abs() < 1e-6, "snapped shift {snapped}");
    }

    #[test]
    fn export_renders_the_corrected_sample() {
        // A slightly flat A4 written to a temp WAV, loaded and analysed like a real session.
        let s = tone(430.0, 0.5, 44100);
        let path = std::env::temp_dir().join("lemon-pitch-export-test.wav");
        std::fs::write(&path, write_wav(&s)).unwrap();
        let shared = Arc::new(Shared::new());
        shared.load(path.to_str().unwrap()).expect("load temp wav");
        let p = PitchParams::default();
        let out = render_corrected(&shared, &p).expect("a corrected render");
        assert_eq!(out.frames(), s.frames(), "duration is preserved");
        let peak = out.left.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(
            peak > 0.05,
            "corrected render should be audible, peak {peak}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn two_tones_make_two_segments() {
        let rate = 44100u32;
        let mut left = Vec::new();
        for (f, secs) in [(440.0f32, 0.4f32), (523.25, 0.4)] {
            let n = (secs * rate as f32) as usize;
            left.extend(
                (0..n).map(|i| (i as f32 / rate as f32 * f * std::f32::consts::TAU).sin() * 0.5),
            );
        }
        let s = Sample {
            rate,
            right: left.clone(),
            left,
        };
        let a = Analysis::of(&s);
        assert!(
            a.segments.len() >= 2,
            "expected >= 2 segments, got {}",
            a.segments.len()
        );
        assert!((a.segments[0].median - 69.0).abs() < 0.3);
        assert!((a.segments[1].median - 72.0).abs() < 0.3);
    }
}
