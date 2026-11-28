//! The song's sample references, which the sample cache decodes before an engine is built. Each sample-playing
//! lane kind appends its source to [`walk`]. A source is a `fn(&Channel, &mut Refs)` that calls `refs.add` for each
//! sample it plays (`refs.add_slot` for a sampler slot). It can call `refs.read` to open a file that names further
//! samples, such as a `.dwp`.

use crate::flm::chunk::{ChunkIter, ContentPath, Cursor};
use crate::flm::sf2d::Sf2d;
use crate::panels::inst::three_osc;
use crate::sample_cache::{Refs, SampleCache, SampleKey};
use crate::song::{Channel, LaneKind, ModuleInstance, Song};
use honeydew_audio::dwp::{Cstm, Zone};
use honeydew_audio::stream::Pcm;

/// Hands every sample `song` plays to `refs`, channel by channel.
pub(crate) fn walk(song: &Song, refs: &mut Refs) {
    for channel in &song.channels {
        drum_slots(channel, refs);
        dw_zones(channel, refs);
        audio_clips(channel, refs);
        soundfonts(channel, refs);
        // Each lane kind appends its source here: audio clips, DW zones …
        slicer_loops(channel, refs);
        custom_shapes(channel, refs);
    }
}

/// A Drums channel's pads: each slot's file or built-in one-shot, with its `STR1` rendered.
fn drum_slots(channel: &Channel, refs: &mut Refs) {
    let slots = channel.drum_kit.iter().flat_map(|kit| &kit.slots);
    slots.for_each(|slot| refs.add_slot(slot));
}

/// Each audio lane clip's `CLSm` slot, muted or not, with its `STR1` rendered.
fn audio_clips(channel: &Channel, refs: &mut Refs) {
    let lanes = channel.lanes.iter().filter(|l| l.kind == LaneKind::Audio);
    let slots = lanes
        .flat_map(|l| &l.clips)
        .filter_map(|c| c.file.sample.as_deref());
    slots.for_each(|slot| refs.add_slot(slot));
}

/// Each DW Sampler's samples.
fn dw_zones(channel: &Channel, refs: &mut Refs) {
    (channel.rack.modules.iter()).for_each(|module| dw_module(module, refs));
}

/// A DW Sampler's samples: the files its `CSTM` `ZONE`s name, and its `PATh` program's zones. Another
/// module has none.
pub(crate) fn dw_module(module: &ModuleInstance, refs: &mut Refs) {
    let blob = module.file.custom_data.as_deref();
    let Some(blob) = blob.filter(|_| module.type_id == 1) else {
        return;
    };
    let cstm = Cstm::read(blob);
    for (path, _) in &cstm.zones {
        if let Some(path) = dw_path(path) {
            refs.add(SampleKey::File(path));
        }
    }
    if let Some(path) = program_path(&cstm) {
        refs.add_program(&path);
    }
}

/// The zones DW Sampler `cstm` plays, each with its decoded sample: its program's (`PATh`/`PATH`) or its `ZONE`s, never
/// both ([`Cstm::read`]); a zone whose sample is missing is dropped. A sampler whose files are all missing
/// plays nothing — FL Studio Mobile names the sample and stays silent, and so does this (no substitute
/// sound is played in its place).
pub(crate) fn sampler_zones(cstm: &Cstm, samples: &SampleCache) -> Vec<(Zone, Pcm)> {
    own_zones(cstm, samples)
}

/// Whether DW Sampler `module` names a sample the loader asked for at the last load and did not
/// find, so it plays nothing for want of that file: FL Studio Mobile's panel then reads "Sample
/// not found or corrupted :(" where its controls would be, and the rack shows the same. A sampler
/// whose program (.dwp) is missing is not this: its panel keeps the values it was saved with (the
/// program's own samples missing does count — the program is there, its sounds are not).
pub(crate) fn dw_missing(module: &ModuleInstance, samples: &SampleCache) -> bool {
    let Some(blob) = module
        .file
        .custom_data
        .as_deref()
        .filter(|_| module.type_id == 1)
    else {
        return false;
    };
    let cstm = Cstm::read(blob);
    // The sample files the module's own zones name.
    let mut keys: Vec<SampleKey> = (cstm.zones.iter())
        .filter_map(|(path, _)| dw_path(path))
        .map(SampleKey::File)
        .collect();
    if let Some(path) = program_path(&cstm) {
        match samples.program(&path) {
            // The program's zones name their samples relative to it; a plain audio file used as
            // the program is its own one zone, keyed by the file itself.
            Some(program) => keys.extend(program.iter().map(|(_, key)| (*key).clone())),
            // No program read: a missing `.dwp` keeps its panel (above), but a program that is a
            // plain audio file and is not there is a missing sample.
            None => {
                let (ContentPath::Shorthand { relative: text, .. } | ContentPath::Plain(text)) =
                    &path;
                let lower = text.to_ascii_lowercase();
                if [".wav", ".flac", ".mp3", ".ogg", ".aif", ".aiff"]
                    .iter()
                    .any(|ext| lower.ends_with(ext))
                {
                    return true;
                }
            }
        }
    }
    !keys.is_empty() && keys.iter().any(|key| samples.missing().any(|m| m == key))
}

fn own_zones(cstm: &Cstm, samples: &SampleCache) -> Vec<(Zone, Pcm)> {
    match program_path(cstm) {
        Some(path) => program_zones(&path, samples, cstm.program.as_ref().and_then(|p| p.root)),
        None => (cstm.zones.iter())
            .filter_map(|(path, zone)| {
                Some((*zone, samples.pcm(&SampleKey::File(dw_path(path)?))?))
            })
            .collect(),
    }
}

fn program_zones(path: &ContentPath, samples: &SampleCache, root: Option<u8>) -> Vec<(Zone, Pcm)> {
    let program = samples.program(path);
    let zones = program.iter().flat_map(|zones| zones.iter());
    zones
        .filter_map(|(zone, key)| Some((rooted(*zone, key, path, root), samples.pcm(key)?)))
        .collect()
}

/// `zone` (its sample `key`) of the program at `path`, as a `PATh` with `root` ([`ProgramRef::root`]) plays it: a
/// plain audio file's one zone, keyed by the program itself, at that root (Honeydew change, #102); a program's zones
/// at their own.
///
/// [`ProgramRef::root`]: honeydew_audio::dwp::ProgramRef::root
pub(crate) fn rooted(zone: Zone, key: &SampleKey, path: &ContentPath, root: Option<u8>) -> Zone {
    match (root, key) {
        (Some(root), SampleKey::File(file)) if file == path => Zone { root, ..zone },
        _ => zone,
    }
}

/// The program DW Sampler `module` plays: its `CSTM` `PATh` (or `PATH`).
pub(crate) fn dw_program(module: &ModuleInstance) -> Option<ContentPath> {
    program_path(&Cstm::read(module.file.custom_data.as_deref()?))
}

/// The program a DW `CSTM` names (`PATh`/`PATH`).
fn program_path(cstm: &Cstm) -> Option<ContentPath> {
    cstm.program
        .as_ref()
        .and_then(|program| dw_path(program.path))
}

/// The content path a DW `CSTM` stores as shorthand bytes, `None` when it is empty.
pub(crate) fn dw_path(raw: &[u8]) -> Option<ContentPath> {
    let vstr = [&(raw.len() as u32).to_le_bytes()[..], raw].concat();
    let path = Cursor::new(&vstr, "CSTM").content_path32().ok()?;
    let (ContentPath::Shorthand { relative: text, .. } | ContentPath::Plain(text)) = &path;
    (!text.is_empty()).then_some(path)
}

/// Each Slicer's loop: its `CSTM` `PATH`, our own `@R@Built-in/Loops/…` ones from code.
fn slicer_loops(channel: &Channel, refs: &mut Refs) {
    (channel.rack.modules.iter())
        .filter_map(super::slicer::loop_path)
        .for_each(|path| refs.add(SampleKey::of_path(&path)));
}

/// Each 3x Osc's custom waveform: its `CSTM` `PATH`, kept as its built table.
fn custom_shapes(channel: &Channel, refs: &mut Refs) {
    (channel.rack.modules.iter())
        .filter_map(shape_key)
        .for_each(|key| refs.add_shape(key));
}

/// A 3x Osc's custom waveform: the path its `CSTM` `PATH` chunk holds. `None` for another module,
/// or a 3x Osc without one.
pub(crate) fn shape_path(module: &ModuleInstance) -> Option<ContentPath> {
    if module.type_id != three_osc::TYPE_ID {
        return None;
    }
    let blob = module.file.custom_data.as_deref()?;
    let mut chunks = ChunkIter::new(blob, "CSTM").map_while(Result::ok);
    let path = chunks
        .find(|c| c.id == *b"PATH")?
        .reader()
        .content_path32()
        .ok()?;
    let (ContentPath::Shorthand { relative: text, .. } | ContentPath::Plain(text)) = &path;
    (!text.is_empty()).then_some(path)
}

/// The sample a 3x Osc's custom waveform plays ([`shape_path`], [`SampleKey::of_path`]): our own
/// `@R@Built-in/Shapes/…` ones from code.
pub(crate) fn shape_key(module: &ModuleInstance) -> Option<SampleKey> {
    shape_path(module).map(|path| SampleKey::of_path(&path))
}

/// The SoundFont each SoundFont Player of the rack names in its `SF2D`.
fn soundfonts(channel: &Channel, refs: &mut Refs) {
    let players = channel
        .rack
        .modules
        .iter()
        .filter(|m| m.type_id == SOUNDFONT_PLAYER);
    players
        .filter_map(|module| Sf2d::read(module.file.custom_data.as_deref()?))
        .for_each(|sf2d| refs.add_soundfont(sf2d.path));
}

/// The SoundFont Player's module type id.
const SOUNDFONT_PLAYER: u16 = 52;

#[cfg(test)]
mod tests {
    use super::*;

    /// A DW Sampler `CSTM`: one zone over every key playing "My Samples/Loop.wav" (as in
    /// `merge`'s tests).
    fn dw_zone() -> Vec<u8> {
        let mut zone = b"@L@\0My Samples/Loop.wav".to_vec();
        zone.resize(1024, 0);
        zone.extend([60, 0, 127, 0, 127]);
        for word in [1.0f32.to_bits(), 0.5f32.to_bits(), 2, 100, 30_000] {
            zone.extend(word.to_le_bytes());
        }
        for value in [0.0f32, 1.0, 1.0, 0.1] {
            zone.extend(value.to_le_bytes());
        }
        let mut blob = b"10WDZONE".to_vec();
        blob.extend((zone.len() as u32).to_le_bytes());
        blob.extend(zone);
        blob
    }

    fn dw_module(blob: Option<Vec<u8>>) -> ModuleInstance {
        let mut module = ModuleInstance::new(1, 2);
        module.file.custom_data = blob;
        module
    }

    /// A 16-bit mono PCM WAV at 44.1 kHz (as in `sample_cache`'s tests).
    fn wav(samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + data.len() as u32).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(44_100u32.to_le_bytes());
        bytes.extend(88_200u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
        bytes
    }

    #[test]
    fn a_dw_sampler_whose_sample_is_missing_is_missing() {
        let module = dw_module(Some(dw_zone()));
        let mut samples = SampleCache::default();
        samples.load_dw(&module, &|_| None);
        assert!(dw_missing(&module, &samples));
    }

    #[test]
    fn a_dw_sampler_whose_sample_is_there_is_not_missing() {
        let module = dw_module(Some(dw_zone()));
        let mut samples = SampleCache::default();
        samples.load_dw(&module, &|_| Some(wav(&[0, 1000, 0, -1000])));
        assert!(!dw_missing(&module, &samples));
    }

    #[test]
    fn a_dw_sampler_that_names_nothing_is_not_missing() {
        let samples = SampleCache::default();
        assert!(!dw_missing(&dw_module(None), &samples));
        assert!(!dw_missing(&dw_module(Some(Vec::new())), &samples));
    }

    #[test]
    fn another_module_with_a_dw_blob_is_not_missing() {
        let samples = SampleCache::default();
        let mut module = ModuleInstance::new(2, 2);
        module.file.custom_data = Some(dw_zone());
        assert!(!dw_missing(&module, &samples));
    }
}
