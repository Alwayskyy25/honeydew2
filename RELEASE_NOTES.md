Honeydew 0.5.6 fixes Citrus volume envelopes silencing operators, adds built-in presets for every effect, and opens the file picker to the full user library.

- Citrus. A switched-on volume envelope no longer silences operators other than OP1 (#180).
- Effects. Every effect module now ships built-in presets — 35 banks and 106 presets, including four for the Leveller (#185).
- File picker. "My Files" shows the full user library — Audio, My MIDI, My Presets, My Racks, My Recordings, My Samples, My Songs, My SoundFonts, My Tracks — like FL Studio Mobile, not just My Samples.
- Tesla. Convert to score can export to the sample library.
- Version. The version shown in the app is now 0.5.6.
- Drums. A kit from an .flm file now lists its slots in pad order, like FL Studio Mobile, not in the order the file stores them.
- DW Sampler. A sampler whose sample is missing now stays silent, like FL Studio Mobile, instead of playing the bundled grand piano.
- Drums. Notes in an .flm song now play the same samples as in FL Studio Mobile — hits follow the kit's records, not just pad order.
- Drum mixer. Each strip now shows a clear stereo level meter under its controls, like FL Studio Mobile.
- Rack. On a portrait screen the rack now follows the UI scale — full width at 100 %, narrower below it — instead of always covering the whole screen.
- Drum channel icon. The drum channel icon is now a 4 × 2 grid of tall pads, each with a slot near its top, like FL Studio Mobile's.
- DW Sampler. A sampler whose sample is missing now shows "Sample not found or corrupted :(" in its panel, like FL Studio Mobile, instead of controls that play nothing.
- Loading. Opening a song now shows a "Loading..." screen until the song is ready, like FL Studio Mobile.
- Menu. The main menu's panel now follows the UI scale in portrait too — narrower below 100 %, leaving part of the playlist visible, like FL Studio Mobile.
- Rack. A side rack's collapse tab now straddles its edge line as a hollow pill, like FL Studio Mobile's.
- Drum clips. Hits in a drum clip now fill their row in the playlist miniature, like the pattern view's steps, instead of hairline dashes that were hard to see.
- Playback. A channel that has been silent for two seconds now sleeps — its instruments and effects stop running until it sounds again — so large projects use far less CPU.
- DW Sampler. Playback now interpolates cubically, like the SoundFont Player's high quality, so a sample pitched down keeps its character instead of rippling.
