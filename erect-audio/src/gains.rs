//! Turning "what is happening" into "how loud is each layer".
//!
//! This is the part worth testing, and it is pure arithmetic: no files, no
//! devices, no sample buffers. Both frontends run exactly this code.

use erect_core::audio::{MusicState, Situation};

/// How a layer decides its target volume.
#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    /// Always at full: the bed the rest sits on.
    Always,
    /// One of a mutually exclusive set. Only the matching situation plays.
    Situation { when: &'static [Situation] },
    /// Volume rises in steps with a live enemy count.
    Count { of: CountOf, steps_db: &'static [(usize, f32)] },
    /// On or off with a flag.
    Boss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountOf {
    Zombie,
    Flyer,
}

#[derive(Clone, Debug)]
pub struct LayerSpec {
    pub id: &'static str,
    pub trigger: Trigger,
    /// Baked into the pack per encoding; re-applied here so both platforms
    /// reproduce the same balance.
    pub file_gain_db: f32,
    pub fade_in_ms: f32,
    pub fade_out_ms: f32,
    /// How long a fall in level has to persist before it is acted on. This is
    /// what stops the volume pumping while an enemy count oscillates around a
    /// step boundary.
    pub hold_ms: f32,
    /// Silenced entirely in these situations.
    pub mute_when: &'static [Situation],
}

impl LayerSpec {
    /// The volume this layer wants, before fading, in linear gain.
    fn target(&self, state: &MusicState) -> f32 {
        if self.mute_when.contains(&state.situation) {
            return 0.0;
        }
        match &self.trigger {
            Trigger::Always => 1.0,
            Trigger::Situation { when } => {
                if when.contains(&state.situation) {
                    1.0
                } else {
                    0.0
                }
            }
            Trigger::Boss => {
                if state.boss {
                    1.0
                } else {
                    0.0
                }
            }
            Trigger::Count { of, steps_db } => {
                let n = match of {
                    CountOf::Zombie => state.zombies,
                    CountOf::Flyer => state.flyers,
                };
                // Highest step whose threshold the count has reached.
                let mut gain = 0.0;
                for (threshold, db) in steps_db.iter() {
                    if n >= *threshold {
                        gain = db_to_linear(*db);
                    }
                }
                gain
            }
        }
    }
}

pub fn db_to_linear(db: f32) -> f32 {
    if db <= -80.0 {
        0.0
    } else {
        libm::powf(10.0, db / 20.0)
    }
}

/// Live gain for one layer: where it is, where it is going, and how fast.
#[derive(Clone, Debug)]
struct LayerState {
    current: f32,
    target: f32,
    /// Countdown before a *decrease* is allowed to take effect.
    hold_left_ms: f32,
    pending: f32,
}

/// Computes per-layer gains over time. Frontends feed it the game state and a
/// delta, and read back one gain per layer.
#[derive(Clone, Debug)]
pub struct GainEngine {
    specs: alloc::vec::Vec<LayerSpec>,
    states: alloc::vec::Vec<LayerState>,
    /// `file_gain_db` as a linear factor. Resolved once: `gain()` is called per
    /// layer per sample, and a `powf` there costs a PSP a quarter of a million
    /// transcendental calls a second.
    file_gain: alloc::vec::Vec<f32>,
}

impl GainEngine {
    pub fn new(specs: alloc::vec::Vec<LayerSpec>) -> Self {
        let states = specs
            .iter()
            .map(|_| LayerState {
                current: 0.0,
                target: 0.0,
                hold_left_ms: 0.0,
                pending: 0.0,
            })
            .collect();
        let file_gain = specs.iter().map(|s| db_to_linear(s.file_gain_db)).collect();
        Self { specs, states, file_gain }
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    pub fn spec(&self, i: usize) -> &LayerSpec {
        &self.specs[i]
    }

    /// Linear gain for layer `i`, including the pack's baked file gain.
    #[inline]
    pub fn gain(&self, i: usize) -> f32 {
        self.states[i].current * self.file_gain[i]
    }

    /// Gain before the file gain, which is what tests want to look at.
    pub fn raw_gain(&self, i: usize) -> f32 {
        self.states[i].current
    }

    pub fn update(&mut self, state: &MusicState, dt_ms: f32) {
        for (i, spec) in self.specs.iter().enumerate() {
            let want = spec.target(state);
            let st = &mut self.states[i];

            if want >= st.target {
                // Rises are immediate: an arriving threat should be heard now.
                st.target = want;
                st.hold_left_ms = 0.0;
                st.pending = want;
            } else if (want - st.pending).abs() > f32::EPSILON {
                // A new, lower level. Start the clock rather than acting at once.
                st.pending = want;
                st.hold_left_ms = spec.hold_ms;
            } else {
                st.hold_left_ms -= dt_ms;
                if st.hold_left_ms <= 0.0 {
                    st.target = want;
                }
            }

            // Fade towards the target at the rate for the direction of travel.
            let ms = if st.target > st.current {
                spec.fade_in_ms
            } else {
                spec.fade_out_ms
            };
            if ms <= 0.0 {
                st.current = st.target;
            } else {
                let step = dt_ms / ms;
                let delta = st.target - st.current;
                if libm::fabsf(delta) <= step {
                    st.current = st.target;
                } else {
                    st.current += if delta > 0.0 { step } else { -step };
                }
            }
            st.current = st.current.clamp(0.0, 1.0);
        }
    }

    /// Jumps every layer straight to its target. Used when starting playback so
    /// the first frame is not a fade-in from silence.
    pub fn snap(&mut self, state: &MusicState) {
        for (i, spec) in self.specs.iter().enumerate() {
            let want = spec.target(state);
            self.states[i] = LayerState {
                current: want,
                target: want,
                hold_left_ms: 0.0,
                pending: want,
            };
        }
    }
}
