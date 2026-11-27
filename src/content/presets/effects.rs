//! Our effect preset banks: three presets per effect module (four for the Leveller), flat in the module's
//! folder, our own sound design and names — the instruments had banks from the start, the effects none, so
//! an effect's preset picker showed only `Default` and the user's own files (issue #185).
//!
//! Values are the registry's normalized ones. Menu values sit mid-step, by each menu's own law
//! ([`crate::params::menu_value`] for `P::menu`, `k / (items − 1)` steps for the `menu_by` selectors).
//! A param whose name the common params share (`Mix`, `Post Gain`) or a module repeats (Tape Stop's two
//! `Stop Time`) is set by `#n`, as the bank format allows. Not covered: the Multiband Compressor (its
//! params are unnamed in the registry), the Tuner and the Analyzer (no audible params), and the legacy
//! modules (5, 10, 12), which only old songs hold.

use super::Preset;

/// An effect preset: no category, no `CSTM` settings.
const fn fx(name: &'static str, params: &'static [(&'static str, f32)]) -> Preset {
    Preset {
        category: "",
        name,
        params,
        settings: &[],
    }
}

/// Every effect bank: a module type and its presets.
pub(super) static BANKS: &[(u16, &[Preset])] = &[
    // ---- 3 Param EQ ----
    (
        3,
        &[
            fx("Vocal Presence", &[("Gain 3", 0.65), ("Gain 4", 0.6)]),
            fx("Bass Boost", &[("Gain 1", 0.68), ("Gain 2", 0.58)]),
            fx("Bright Air", &[("Gain 3", 0.58), ("Gain 4", 0.7)]),
        ],
    ),
    // ---- 4 Multi FX ---- (`FX Kind` by trunc(7v) over 8 kinds; `LFO Rate` by trunc(9v) over 10.)
    (
        4,
        &[
            fx(
                "Echo Space",
                &[("FX Kind", 0.071), ("Axis X", 0.6), ("Axis Y", 0.4)],
            ),
            fx(
                "Wash Reverb",
                &[("FX Kind", 0.214), ("Axis X", 0.5), ("Axis Y", 0.65)],
            ),
            fx(
                "Auto Pan",
                &[
                    ("FX Kind", 0.929),
                    ("LFO Rate", 0.611),
                    ("LFO Amount", 0.7),
                    ("LFO Shape", 0.25),
                ],
            ),
        ],
    ),
    // ---- 6 Distortion ----
    (
        6,
        &[
            fx("Soft Crunch", &[("Gain", 0.35), ("Bias", 0.5)]),
            fx("Vintage Dirt", &[("Gain", 0.6), ("Bias", 0.4)]),
            fx("Max Grit", &[("Gain", 1.0), ("Bias", 0.65)]),
        ],
    ),
    // ---- 7 Graphic EQ ----
    (
        7,
        &[
            fx(
                "Smile",
                &[
                    ("80 Hz", 0.62),
                    ("160 Hz", 0.56),
                    ("375 Hz", 0.47),
                    ("750 Hz", 0.45),
                    ("1.5 kHz", 0.5),
                    ("3 kHz", 0.56),
                    ("6 kHz", 0.62),
                    ("12 kHz", 0.66),
                ],
            ),
            fx(
                "Bass Cut",
                &[("80 Hz", 0.3), ("160 Hz", 0.38), ("375 Hz", 0.45)],
            ),
            fx(
                "Vocal",
                &[
                    ("375 Hz", 0.42),
                    ("750 Hz", 0.56),
                    ("1.5 kHz", 0.64),
                    ("3 kHz", 0.6),
                    ("6 kHz", 0.52),
                ],
            ),
        ],
    ),
    // ---- 8 Compressor ----
    (
        8,
        &[
            fx(
                "Gentle Glue",
                &[
                    ("Thresh", 0.55),
                    ("Ratio", 0.25),
                    ("Attack", 0.5),
                    ("Release", 0.55),
                ],
            ),
            fx(
                "Punch",
                &[
                    ("Thresh", 0.45),
                    ("Ratio", 0.5),
                    ("Attack", 0.35),
                    ("Release", 0.4),
                ],
            ),
            fx(
                "Vocal Leveller",
                &[
                    ("Thresh", 0.4),
                    ("Ratio", 0.65),
                    ("Attack", 0.45),
                    ("Release", 0.5),
                ],
            ),
        ],
    ),
    // ---- 9 Filter ---- (its registry param is the cutoff alone)
    (
        9,
        &[
            fx("Mellow Cut", &[("Frequency", 0.3)]),
            fx("Mid Point", &[("Frequency", 0.55)]),
            fx("Full Open", &[("Frequency", 0.95)]),
        ],
    ),
    // ---- 11 Chorus ---- (`Mix` by index: the common params hold a `Mix` too)
    (
        11,
        &[
            fx("Lush Wash", &[("Rate", 0.3), ("Depth", 0.8), ("#4", 0.7)]),
            fx(
                "Subtle Shine",
                &[("Rate", 0.45), ("Depth", 0.35), ("#4", 0.35)],
            ),
            fx("Deep Swirl", &[("Rate", 0.6), ("Depth", 0.95), ("#4", 0.8)]),
        ],
    ),
    // ---- 13 Flanger ----
    (
        13,
        &[
            fx(
                "Jet Sweep",
                &[
                    ("LFO Speed", 0.35),
                    ("Delay", 0.3),
                    ("Feedback", 0.85),
                    ("#10", 0.8),
                ],
            ),
            fx(
                "Metallic",
                &[
                    ("LFO Speed", 0.5),
                    ("Delay", 0.12),
                    ("Feedback", 0.95),
                    ("#10", 0.7),
                ],
            ),
            fx(
                "Gentle Wave",
                &[
                    ("LFO Speed", 0.25),
                    ("Delay", 0.4),
                    ("Feedback", 0.5),
                    ("#10", 0.5),
                ],
            ),
        ],
    ),
    // ---- 14 Phaser ----
    (
        14,
        &[
            fx(
                "Slow Sweep",
                &[("LFO Speed", 0.2), ("LFO Amount", 0.6), ("Feedback", 0.4)],
            ),
            fx(
                "Funky",
                &[
                    ("LFO Speed", 0.55),
                    ("LFO Amount", 0.8),
                    ("Feedback", 0.6),
                    ("Frequency", 0.4),
                ],
            ),
            fx(
                "Deep Space",
                &[("LFO Speed", 0.12), ("LFO Amount", 1.0), ("Feedback", 0.8)],
            ),
        ],
    ),
    // ---- 15 Stereoizer ----
    (
        15,
        &[
            fx("Wide Stage", &[("Focus", 0.9), ("L/R Delay", 0.62)]),
            fx("Mono Focus", &[("Focus", 0.2), ("L/R Delay", 0.5)]),
            fx("Subtle Width", &[("Focus", 0.6), ("L/R Delay", 0.55)]),
        ],
    ),
    // ---- 16 Trance Delay ---- (`Time` by trunc(4v) over 5 steps: 1/4, 3/8, 1/2, 3/4, 1/1)
    (
        16,
        &[
            fx(
                "Quarter Echo",
                &[("Time", 0.125), ("Fdbk", 0.5), ("#3", 0.5)],
            ),
            fx(
                "Dotted Groove",
                &[("Time", 0.375), ("Fdbk", 0.65), ("#3", 0.55)],
            ),
            fx(
                "Half Wash",
                &[
                    ("Time", 0.625),
                    ("Fdbk", 0.75),
                    ("#3", 0.6),
                    ("Filter", 0.35),
                ],
            ),
        ],
    ),
    // ---- 18 Tape Delay ---- (`Time` reads 1000·v² ms)
    (
        18,
        &[
            fx("Slapback", &[("Time", 0.32), ("Fdbk", 0.25), ("#4", 0.35)]),
            fx("Dub Echo", &[("Time", 0.77), ("Fdbk", 0.8), ("#4", 0.5)]),
            fx(
                "Wobble Space",
                &[
                    ("Time", 0.6),
                    ("Fdbk", 0.65),
                    ("Async.", 0.62),
                    ("#4", 0.45),
                ],
            ),
        ],
    ),
    // ---- 20 Leveller ---- (`Gain` reads 72·v^0.6 − 60 dB: 0 dB at 0.738, +6 dB at 0.865, −6 dB at 0.619)
    (
        20,
        &[
            fx("Boost +6 dB", &[("Gain", 0.865)]),
            fx("Cut -6 dB", &[("Gain", 0.619)]),
            fx("Max Boost", &[("Gain", 1.0)]),
            fx("Soft Left", &[("Gain", 0.738), ("Pan", 0.15)]),
        ],
    ),
    // ---- 25 Pitcher ---- (its params are unnamed in the registry: #2 Pitch reads v·24 − 12 semitones)
    (
        25,
        &[
            fx("Octave Up", &[("#2", 1.0)]),
            fx("Octave Down", &[("#2", 0.0)]),
            fx("Fifth Up", &[("#2", 0.7917)]),
        ],
    ),
    // ---- 27 Gate ----
    (
        27,
        &[
            fx(
                "Tight Chops",
                &[
                    ("Threshold", 0.35),
                    ("Attack", 0.0),
                    ("Release", 0.15),
                    ("Hold", 0.1),
                ],
            ),
            fx(
                "Soft Gate",
                &[("Threshold", 0.18), ("Attack", 0.05), ("Release", 0.4)],
            ),
            fx(
                "Stutter",
                &[("Threshold", 0.5), ("Release", 0.08), ("Hold", 0.05)],
            ),
        ],
    ),
    // ---- 28 Limiter ----
    (
        28,
        &[
            fx(
                "Master Ceiling",
                &[
                    ("In Gain", 0.6),
                    ("Out Gain", 0.45),
                    ("Threshold", 0.9),
                    ("Release Time", 0.35),
                ],
            ),
            fx(
                "Loud and Proud",
                &[("In Gain", 0.75), ("Out Gain", 0.5), ("Threshold", 0.85)],
            ),
            fx(
                "Gentle Cap",
                &[
                    ("In Gain", 0.5),
                    ("Out Gain", 0.5),
                    ("Threshold", 0.95),
                    ("Release Time", 0.5),
                ],
            ),
        ],
    ),
    // ---- 29 Autoduck ----
    (
        29,
        &[
            fx(
                "Sidechain Pump",
                &[
                    ("Ducking", 0.85),
                    ("Attack", 0.05),
                    ("Time", 0.7),
                    ("Hold", 0.2),
                ],
            ),
            fx(
                "Gentle Dip",
                &[("Ducking", 0.35), ("Attack", 0.2), ("Time", 0.85)],
            ),
            fx(
                "Hard Duck",
                &[
                    ("Ducking", 1.0),
                    ("Attack", 0.0),
                    ("Time", 0.55),
                    ("Hold", 0.35),
                ],
            ),
        ],
    ),
    // ---- 30 Tuned EQ ----
    (
        30,
        &[
            fx(
                "Warmth",
                &[
                    ("65 Hz", 0.62),
                    ("130 Hz", 0.58),
                    ("261 Hz", 0.53),
                    ("4.2 kHz", 0.46),
                    ("8.4 kHz", 0.44),
                ],
            ),
            fx(
                "Clarity",
                &[
                    ("261 Hz", 0.44),
                    ("523 Hz", 0.45),
                    ("1.0 kHz", 0.58),
                    ("2.1 kHz", 0.62),
                    ("4.2 kHz", 0.58),
                ],
            ),
            fx(
                "Bass Roll-off",
                &[("65 Hz", 0.25), ("130 Hz", 0.38), ("261 Hz", 0.47)],
            ),
        ],
    ),
    // ---- 32 Reverb ----
    (
        32,
        &[
            fx(
                "Small Room",
                &[
                    ("DECAY", 0.25),
                    ("ROOM SIZE", 0.4),
                    ("MIX", 0.3),
                    ("EARLY REFL.", 0.4),
                ],
            ),
            fx(
                "Big Hall",
                &[
                    ("DECAY", 0.75),
                    ("ROOM SIZE", 1.0),
                    ("MIX", 0.45),
                    ("PREDELAY", 0.15),
                ],
            ),
            fx(
                "Long Wash",
                &[
                    ("DECAY", 0.95),
                    ("MIX", 0.55),
                    ("HIGH DAMP", 0.5),
                    ("DIFFUSION", 1.0),
                ],
            ),
        ],
    ),
    // ---- 34 Spacer ----
    (
        34,
        &[
            fx("Wide Sky", &[("Separation", 0.85), ("L/R Delay", 0.6)]),
            fx("Narrow", &[("Separation", 0.2)]),
            fx(
                "Extra Wide",
                &[("Separation", 1.0), ("L/R Delay", 0.68), ("M/S Delay", 0.6)],
            ),
        ],
    ),
    // ---- 35 Reverb 2 ----
    (
        35,
        &[
            fx(
                "Cathedral",
                &[("SIZE", 0.9), ("DECAY", 0.35), ("WET", 0.7), ("DRY", 0.7)],
            ),
            fx(
                "Tight Room",
                &[("SIZE", 0.25), ("DECAY", 0.03), ("WET", 0.4), ("ER", 0.9)],
            ),
            fx(
                "Shimmer Wash",
                &[
                    ("SIZE", 0.7),
                    ("DECAY", 0.5),
                    ("WET", 0.8),
                    ("MOD", 0.4),
                    ("HIGH CUT", 0.3),
                ],
            ),
        ],
    ),
    // ---- 40 Comb Filter ----
    (
        40,
        &[
            fx(
                "Metallic Ring",
                &[("Pitch", 0.45), ("Feedback", 0.9), ("#4", 0.6)],
            ),
            fx(
                "Hollow Tube",
                &[("Pitch", 0.25), ("Feedback", 0.7), ("#4", 0.65)],
            ),
            fx(
                "Subtle Resonance",
                &[("Pitch", 0.55), ("Feedback", 0.45), ("#4", 0.35)],
            ),
        ],
    ),
    // ---- 41 Waveshaper ---- (`Post Gain` is #6 by index: the common params hold one too)
    (
        41,
        &[
            fx(
                "Warmth",
                &[
                    ("Bass", 0.65),
                    ("Mid Gain", 0.55),
                    ("Treble", 0.4),
                    ("#6", 0.85),
                ],
            ),
            fx(
                "Bright Edge",
                &[("Treble", 0.7), ("Mid Gain", 0.6), ("Bass", 0.45)],
            ),
            fx(
                "Dark Grit",
                &[
                    ("Bass", 0.7),
                    ("Treble", 0.3),
                    ("High Cut", 0.6),
                    ("#6", 0.9),
                ],
            ),
        ],
    ),
    // ---- 42 Tape Stop ---- (its two `Stop Time` / `Start Time` pairs are set by index)
    (
        42,
        &[
            fx("Fast Stop", &[("#1", 0.1), ("#2", 0.1)]),
            fx("Slow Stop", &[("#1", 0.45), ("#2", 0.45)]),
            fx("Vinyl Drag", &[("#1", 0.7), ("#2", 0.3), ("#6", 0.7)]),
        ],
    ),
    // ---- 43 Wow & Flutter ---- (`Wow Rate` is #1 by index: the free and synced rates share the name)
    (
        43,
        &[
            fx(
                "Old Tape",
                &[("Wow Amount", 0.5), ("Flutter Amount", 0.4), ("#1", 0.22)],
            ),
            fx(
                "Warped Vinyl",
                &[("Wow Amount", 0.8), ("#1", 0.3), ("Random Amount", 0.4)],
            ),
            fx(
                "Subtle Drift",
                &[("Wow Amount", 0.12), ("Flutter Amount", 0.1)],
            ),
        ],
    ),
    // ---- 44 Spreader ----
    (
        44,
        &[
            fx("Wide Mix", &[("Width", 0.7), ("Separation", 0.65)]),
            fx(
                "Full Spread",
                &[("Width", 1.0), ("Separation", 0.8), ("Low Bypass", 0.3)],
            ),
            fx("Gentle Width", &[("Width", 0.35), ("Separation", 0.55)]),
        ],
    ),
    // ---- 45 Pitcher 2 ----
    (
        45,
        &[
            fx(
                "Natural Tune",
                &[
                    ("Snapping", 0.4),
                    ("Sensitivity", 0.6),
                    ("Humanization", 0.3),
                ],
            ),
            fx(
                "Hard Tune",
                &[
                    ("Snapping", 0.9),
                    ("Sensitivity", 0.85),
                    ("Humanization", 0.0),
                    ("Stability", 0.7),
                ],
            ),
            fx(
                "Soft Correction",
                &[
                    ("Snapping", 0.2),
                    ("Sensitivity", 0.45),
                    ("Humanization", 0.5),
                ],
            ),
        ],
    ),
    // ---- 46 Tremolo ----
    (
        46,
        &[
            fx(
                "Slow Pulse",
                &[
                    ("Rate Free", 0.08),
                    ("Amount Max", 1.0),
                    ("Amount Min", 0.1),
                ],
            ),
            fx("Fast Chop", &[("Rate Free", 0.3), ("Amount Min", 0.0)]),
            fx(
                "Stereo Sway",
                &[
                    ("Rate Free", 0.12),
                    ("Stereo Offset", 0.5),
                    ("Amount Max", 0.8),
                ],
            ),
        ],
    ),
    // ---- 47 Note Echo ---- (`Repetitions` reads 32 steps; `Pitch` reads 24·v − 12 semitones per repeat)
    (
        47,
        &[
            fx(
                "Ping Pong Trail",
                &[
                    ("Ping-Pong", 1.0),
                    ("Repetitions", 0.375),
                    ("Velocity", 0.35),
                ],
            ),
            fx(
                "Falling Echoes",
                &[("Repetitions", 0.5), ("Pitch", 0.4583), ("Velocity", 0.4)],
            ),
            fx(
                "Rising Echoes",
                &[("Repetitions", 0.5), ("Pitch", 0.5417), ("Velocity", 0.45)],
            ),
        ],
    ),
    // ---- 51 Arpeggiator ---- (menus by round(v·(items − 1)): `Arp` Up 2/23, Up-Down 4/23, Random 20/23)
    (
        51,
        &[
            fx(
                "Up Run",
                &[("Arp (Arp 1)", 0.087), ("Range (Arp 1)", 0.333)],
            ),
            fx(
                "Up-Down Wave",
                &[("Arp (Arp 1)", 0.174), ("Range (Arp 1)", 0.667)],
            ),
            fx("Random Sparkle", &[("Arp (Arp 1)", 0.87), ("Rate", 0.143)]),
        ],
    ),
    // ---- 1004 Dynamic EQ ---- (band thresholds read −60…0 dB, gains and ranges −18…+18 dB)
    (
        1004,
        &[
            fx(
                "De-Harsh",
                &[("Band 3 Threshold", 0.583), ("Band 3 Range", 0.333)],
            ),
            fx(
                "Bass Tamer",
                &[("Band 1 Threshold", 0.583), ("Band 1 Range", 0.333)],
            ),
            fx(
                "Presence Lift",
                &[("Band 4 Gain", 0.583), ("Band 4 Threshold", 1.0)],
            ),
        ],
    ),
    // ---- 1005 Modulator ---- (`Source` by round(2v): Envelope 0, LFO 0.5, Random 1)
    (
        1005,
        &[
            fx(
                "Auto Swell",
                &[
                    ("Source", 0.0),
                    ("Amount", 0.875),
                    ("Attack", 0.5),
                    ("Release", 0.66),
                ],
            ),
            fx(
                "LFO Wobble",
                &[
                    ("Source", 0.5),
                    ("Rate", 0.35),
                    ("Amount", 0.65),
                    ("Smooth", 0.5),
                ],
            ),
            fx(
                "Random Steps",
                &[
                    ("Source", 1.0),
                    ("Sync", 1.0),
                    ("Rate Sync", 0.286),
                    ("Amount", 0.75),
                    ("Smooth", 0.1),
                ],
            ),
        ],
    ),
    // ---- 1006 De-Esser ---- (threshold reads −60…0 dB, range 0…24 dB)
    (
        1006,
        &[
            fx("Gentle Tame", &[("Threshold", 0.583), ("Range", 0.333)]),
            fx(
                "Strong Sibilance",
                &[("Threshold", 0.417), ("Range", 0.75), ("Frequency", 0.7)],
            ),
            fx(
                "Wide Mode",
                &[("Mode", 1.0), ("Threshold", 0.5), ("Range", 0.5)],
            ),
        ],
    ),
    // ---- 1007 Denoise ---- (reduction reads 0…40 dB)
    (
        1007,
        &[
            fx("Light Clean", &[("Reduction", 0.2), ("Sensitivity", 0.4)]),
            fx("Heavy Clean", &[("Reduction", 0.6), ("Sensitivity", 0.6)]),
            fx(
                "Voice Polish",
                &[("Reduction", 0.35), ("Sensitivity", 0.55)],
            ),
        ],
    ),
    // ---- 1008 Half Time ---- (`Length` by round(7v) over 8 lengths, `Speed` by round(2v): 2/3, 1/2, 1/4)
    (
        1008,
        &[
            fx(
                "Classic Half",
                &[("Length", 0.571), ("Speed", 0.5), ("#2", 1.0)],
            ),
            fx(
                "Deep Slow",
                &[("Length", 0.714), ("Speed", 1.0), ("#2", 1.0)],
            ),
            fx(
                "Half Blend",
                &[("Length", 0.429), ("Speed", 0.5), ("#2", 0.5)],
            ),
        ],
    ),
];
