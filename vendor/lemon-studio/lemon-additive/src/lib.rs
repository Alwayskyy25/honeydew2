// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lemon Additive — an additive synthesizer in the style of FL Studio's Harmor, designed and written from
// scratch from public documentation of the original's feature set. No original code or assets are used.

mod dsp;
mod editor;
mod params;
mod view;

use std::sync::Arc;

use nih_plug::prelude::*;

pub use dsp::Engine;
pub use params::AdditiveParams;

pub struct LemonAdditive {
    params: Arc<AdditiveParams>,
    engine: Engine,
    sample_rate: f32,
    /// Pitch bend in semitones.
    bend: f32,
}

impl Default for LemonAdditive {
    fn default() -> Self {
        LemonAdditive { params: Arc::new(AdditiveParams::default()), engine: Engine::new(), sample_rate: 44100.0, bend: 0.0 }
    }
}

impl Plugin for LemonAdditive {
    const NAME: &'static str = "Lemon Additive";
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
        Some(Box::new(editor::AdditiveEditor::new(self.params.clone())))
    }

    fn initialize(&mut self, _audio_io_layout: &AudioIOLayout, buffer_config: &BufferConfig, _context: &mut impl InitContext<Self>) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.engine.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.engine.all_notes_off();
        self.bend = 0.0;
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let frames = buffer.samples();
        if frames == 0 {
            return ProcessStatus::Normal;
        }
        while let Some(ev) = context.next_event() {
            match ev {
                NoteEvent::NoteOn { note, channel, velocity, .. } => self.engine.note_on(&self.params, note, channel, velocity),
                NoteEvent::NoteOff { note, channel, .. } => self.engine.note_off(note, channel),
                NoteEvent::MidiPitchBend { value, .. } => self.bend = (value - 0.5) * 4.0,
                NoteEvent::MidiCC { cc, value, .. } if cc == 120 || cc == 123 => {
                    let _ = value;
                    self.engine.all_notes_off();
                }
                _ => {}
            }
        }
        // Render into planar scratch, then copy into the buffer's channels.
        let mut lb = vec![0.0f32; frames];
        let mut rb = vec![0.0f32; frames];
        self.engine.render(&self.params, self.bend, &mut lb, &mut rb);
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

impl ClapPlugin for LemonAdditive {
    const CLAP_ID: &'static str = "com.lemon-studio.lemon-additive";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Additive synthesizer with two parts and a drawable harmonic spectrum");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
}

os_plugkit::linked_clap_entry!(LemonAdditive);
