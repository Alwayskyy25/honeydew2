// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Pitch — a pitch-correction editor instrument in the style of FL Studio's Newtone, designed and
// written from scratch from public documentation of the original's feature set. No original code or assets
// are used.

mod dsp;
mod editor;
mod params;
mod view;

use std::sync::Arc;

use nih_plug::prelude::*;

pub use dsp::{Analysis, Engine, Segment, Shared};
pub use params::PitchParams;

pub struct LemonPitch {
    params: Arc<PitchParams>,
    shared: Arc<Shared>,
    engine: Engine,
    sample_rate: f32,
    /// The sample path the shared state was last synced with the persisted parameter.
    synced_path: String,
}

impl Default for LemonPitch {
    fn default() -> Self {
        let shared = Arc::new(Shared::new());
        LemonPitch {
            params: Arc::new(PitchParams::default()),
            engine: Engine::new(shared.clone()),
            shared,
            sample_rate: 44100.0,
            synced_path: String::new(),
        }
    }
}

impl Plugin for LemonPitch {
    const NAME: &'static str = "Lemon Pitch";
    const VENDOR: &'static str = "Lemon Studio";
    const URL: &'static str = "https://github.com/ItsAlphaNeon/Lemon-Studio";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout { main_input_channels: None, main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() }];
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::PitchEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _audio_io_layout: &AudioIOLayout, buffer_config: &BufferConfig, _context: &mut impl InitContext<Self>) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.engine.set_sample_rate(buffer_config.sample_rate);
        self.sync_path();
        true
    }

    fn reset(&mut self) {
        self.engine.all_notes_off();
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let frames = buffer.samples();
        if frames == 0 {
            return ProcessStatus::Normal;
        }
        self.sync_path();
        while let Some(ev) = context.next_event() {
            match ev {
                NoteEvent::NoteOn { note, channel, velocity, .. } => self.engine.note_on(note, channel, velocity),
                NoteEvent::NoteOff { note, channel, .. } => self.engine.note_off(note, channel),
                NoteEvent::MidiCC { cc, .. } if cc == 120 || cc == 123 => self.engine.all_notes_off(),
                _ => {}
            }
        }
        let mut lb = vec![0.0f32; frames];
        let mut rb = vec![0.0f32; frames];
        self.engine.render(&self.params, &mut lb, &mut rb);
        let chans = buffer.as_slice();
        if !chans.is_empty() {
            chans[0][..frames].copy_from_slice(&lb);
        }
        if chans.len() >= 2 {
            chans[1][..frames].copy_from_slice(&rb);
        }
        ProcessStatus::Normal
    }
}

impl LemonPitch {
    /// Keeps the shared sample in step with the persisted `sample-path` parameter (a restored project, or a
    /// path the editor just chose — the editor also writes the parameter when it loads).
    fn sync_path(&mut self) {
        let want = self.params.sample_path.read().map(|p| p.clone()).unwrap_or_default();
        if want != self.synced_path {
            self.synced_path = want.clone();
            if !want.is_empty() && self.shared.loaded_path() != want {
                let _ = self.shared.load(&want);
            }
        }
    }
}

impl ClapPlugin for LemonPitch {
    const CLAP_ID: &'static str = "com.lemon-studio.lemon-pitch";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Pitch editor instrument: per-segment pitch correction of a loaded vocal or melody sample");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Sampler, ClapFeature::Stereo];
}

os_plugkit::linked_clap_entry!(LemonPitch);
