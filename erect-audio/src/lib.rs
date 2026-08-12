//! Adaptive music and sound mixing for E-Rect.
//!
//! The music is one composition split into layers that all play at once from a
//! single shared playhead; what changes with the game is only how loud each
//! layer is. That keeps them in sync by construction and lets a layer appear
//! mid-phrase already in time.
//!
//! Nothing here touches a file or a sound device - frontends hand over decoded
//! samples and take back mixed blocks.

#![no_std]

extern crate alloc;

pub mod gains;
pub mod mixer;

#[cfg(test)]
mod tests;

pub use gains::{CountOf, GainEngine, LayerSpec, Trigger};
pub use mixer::{volume_to_gain, Mixer, Samples, SfxSpec};

/// The shipped packs, as constants rather than a parsed manifest.
///
/// Both platforms need the same numbers, and a PSP has no business carrying a
/// JSON parser to learn them. Each `pack.json` remains the authoring record;
/// this is what the game runs on.
pub mod packs {
    use erect_core::audio::Situation;

    use crate::gains::{CountOf, LayerSpec, Trigger};

    pub const SAMPLE_RATE: u32 = 44100;

    /// Roles, in the order the mixer is handed layer data. Every pack uses these
    /// names whatever its source files were called - the build tool renames them
    /// on the way out, so the runtime never has to know.
    pub const LAYER_IDS: [&str; 6] =
        ["all_time", "leisure", "fight", "zombie", "garp", "boss"];

    const CALM_OR_PAUSED: [Situation; 2] = [Situation::Calm, Situation::Paused];
    const COMBAT: [Situation; 1] = [Situation::Combat];
    const PAUSED: [Situation; 1] = [Situation::Paused];
    const NEVER: [Situation; 0] = [];

    /// One shipped set of music and sounds.
    pub struct Pack {
        /// Directory name under `packs/`, and under the pack folder on a PSP.
        pub dir: &'static str,
        pub loop_samples: usize,
        /// Gain the build baked out of each layer, per encoding. The desktop
        /// keeps the levels as mixed; the PSP normalises every file to full
        /// scale for its 8-bit depth and hands the difference back here.
        pub desktop_gains_db: [f32; 6],
        pub psp_gains_db: [f32; 6],
        /// `(id, max_voices, min_interval_ms, psp_gain_db)`
        pub sfx: [(&'static str, usize, f32, f32); 3],
    }

    /// Both packs ship the same three sounds; only the music differs.
    const SFX: [(&str, usize, f32, f32); 3] = [
        ("hit", 4, 40.0, -15.36),
        ("jump", 2, 60.0, -7.82),
        ("down", 3, 60.0, -12.52),
    ];

    pub static PACKS: [Pack; 2] = [
        Pack {
            dir: "pack1",
            loop_samples: 991232,
            desktop_gains_db: [0.0; 6],
            psp_gains_db: [-7.50, -27.08, -8.69, -18.06, -16.20, -22.28],
            sfx: SFX,
        },
        Pack {
            dir: "pack2",
            loop_samples: 1088640,
            desktop_gains_db: [0.0; 6],
            psp_gains_db: [-10.80, -18.59, -12.17, -10.20, -11.92, -17.63],
            sfx: SFX,
        },
    ];

    /// Picks a pack for this run. Called once at startup, so a restart is what
    /// rolls a new one - swapping mid-run would mean reloading megabytes of
    /// audio and restarting the shared playhead mid-phrase.
    ///
    /// The seed is hashed before the modulus rather than after. It comes from a
    /// clock, and taking `seed % 2` reads only the bottom bit - which on a PSP
    /// barely moves between launches. Four runs in a row drew the same pack
    /// before this went in.
    pub fn choose(seed: u64) -> &'static Pack {
        &PACKS[(mix(seed) % PACKS.len() as u64) as usize]
    }

    /// splitmix64's finaliser: every input bit reaches every output bit, so a
    /// clock that only ticks its high bits still spreads.
    fn mix(seed: u64) -> u64 {
        let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^ (x >> 31)
    }

    pub fn layers(gains_db: [f32; 6]) -> alloc::vec::Vec<LayerSpec> {
        alloc::vec![
            LayerSpec {
                id: "all_time",
                trigger: Trigger::Always,
                file_gain_db: gains_db[0],
                fade_in_ms: 400.0,
                fade_out_ms: 400.0,
                hold_ms: 0.0,
                mute_when: &NEVER,
            },
            LayerSpec {
                id: "leisure",
                trigger: Trigger::Situation { when: &CALM_OR_PAUSED },
                file_gain_db: gains_db[1],
                fade_in_ms: 700.0,
                fade_out_ms: 700.0,
                hold_ms: 0.0,
                mute_when: &NEVER,
            },
            LayerSpec {
                id: "fight",
                trigger: Trigger::Situation { when: &COMBAT },
                file_gain_db: gains_db[2],
                fade_in_ms: 700.0,
                fade_out_ms: 700.0,
                hold_ms: 0.0,
                mute_when: &NEVER,
            },
            LayerSpec {
                id: "zombie",
                trigger: Trigger::Count {
                    of: CountOf::Zombie,
                    steps_db: &[(1, -12.0), (2, -6.0), (3, 0.0)],
                },
                file_gain_db: gains_db[3],
                fade_in_ms: 250.0,
                fade_out_ms: 900.0,
                hold_ms: 700.0,
                mute_when: &PAUSED,
            },
            LayerSpec {
                id: "garp",
                trigger: Trigger::Count {
                    of: CountOf::Flyer,
                    steps_db: &[(1, -12.0), (2, -6.0), (3, 0.0)],
                },
                file_gain_db: gains_db[4],
                fade_in_ms: 250.0,
                fade_out_ms: 900.0,
                hold_ms: 700.0,
                mute_when: &PAUSED,
            },
            LayerSpec {
                id: "boss",
                trigger: Trigger::Boss,
                file_gain_db: gains_db[5],
                fade_in_ms: 120.0,
                fade_out_ms: 1500.0,
                hold_ms: 0.0,
                mute_when: &PAUSED,
            },
        ]
    }
}
