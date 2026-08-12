//! One-shot sound cues the simulation emits, and the music situation it implies.
//!
//! The core does not know what a sound *is*. It reports that something happened
//! and what the fight currently looks like; a frontend maps that onto whatever
//! the sound pack provides.

use alloc::vec::Vec;

/// A thing that just happened and wants a sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEvent {
    /// A player swung. Not "a hit landed" - the swing is what the player feels.
    Hit,
    Jump,
    /// The slam landed and put up the wall. Not every landing: an ordinary
    /// touchdown is silent, only the wall effect is heard.
    Slam,
}

/// Which of the mutually exclusive background themes belongs to this moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Situation {
    /// Menus and the lull between waves.
    Calm,
    Combat,
    Paused,
}

/// Everything the music needs to pick its layer volumes this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicState {
    pub situation: Situation,
    pub zombies: usize,
    pub flyers: usize,
    pub boss: bool,
}

/// Collected during a tick, drained by the frontend afterwards.
#[derive(Clone, Debug, Default)]
pub struct AudioQueue {
    events: Vec<AudioEvent>,
}

impl AudioQueue {
    pub fn push(&mut self, event: AudioEvent) {
        // A tick is 1/60 s; more than a handful of identical cues in one tick
        // would only phase against each other anyway.
        if self.events.len() < 16 {
            self.events.push(event);
        }
    }

    pub fn drain(&mut self) -> impl Iterator<Item = AudioEvent> + '_ {
        self.events.drain(..)
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}
