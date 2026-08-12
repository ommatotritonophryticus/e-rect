//! Player choices and high scores, as plain data.
//!
//! Deliberately no file access: a desktop writes JSON to a config directory, a
//! PSP writes bytes to the Memory Stick. The core only tracks the values and
//! raises [`Settings::dirty`] when a frontend should persist them.

use crate::config::{MAX_PLAYERS, PLAYER_COLORS};

/// One control scheme a platform offers. Supplied by the frontend, because the
/// list differs per platform - a desktop has keyboard layouts and gamepads, a
/// PSP has exactly one pad.
#[derive(Clone, Copy, Debug)]
pub struct SchemeInfo {
    pub label: &'static str,
    pub is_gamepad: bool,
    /// Which physical pad a gamepad scheme reads.
    pub pad_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerConfig {
    pub scheme: usize,
    pub color_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub players: [PlayerConfig; MAX_PLAYERS],
    pub record_solo: i64,
    pub record_duo: i64,
    /// 0..=100 in steps of 10. Kept as a percentage rather than a gain so the
    /// menu, the save file and the mixer never disagree about what "50" means;
    /// turning it into a gain is the audio layer's job.
    pub music_volume: u32,
    pub sfx_volume: u32,
    /// How many times this install has been started. Only there to seed the
    /// random number generator on platforms whose clock does not move between
    /// launches - a PSP emulator reports the same uptime every single run.
    pub launches: u64,
    /// Set whenever something changed; the frontend clears it after saving.
    pub dirty: bool,
}

/// Which of the two volume rows is being adjusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeChannel {
    Music,
    Sfx,
}

pub const VOLUME_STEP: u32 = 10;
pub const VOLUME_MAX: u32 = 100;

impl Default for Settings {
    fn default() -> Self {
        Self {
            players: [
                PlayerConfig {
                    scheme: 0,
                    color_index: 0,
                },
                PlayerConfig {
                    scheme: 1,
                    color_index: 1,
                },
            ],
            record_solo: 0,
            record_duo: 0,
            music_volume: VOLUME_MAX,
            sfx_volume: VOLUME_MAX,
            launches: 0,
            dirty: false,
        }
    }
}

impl Settings {
    /// Clamps anything a stale or hand-edited save file might contain, so bad
    /// input can never crash startup. `scheme_count` is however many schemes
    /// this platform actually offers.
    pub fn sanitize(&mut self, scheme_count: usize) {
        let defaults = Settings::default();
        let scheme_count = scheme_count.max(1);

        for i in 0..MAX_PLAYERS {
            if self.players[i].scheme >= scheme_count {
                self.players[i].scheme = defaults.players[i].scheme.min(scheme_count - 1);
            }
            if self.players[i].color_index >= PLAYER_COLORS.len() {
                self.players[i].color_index = defaults.players[i].color_index;
            }
        }

        // Sharing a colour would make the two players indistinguishable.
        if self.players[0].color_index == self.players[1].color_index {
            self.players[1].color_index =
                (self.players[0].color_index + 1) % PLAYER_COLORS.len();
        }
        // Sharing a scheme is only a problem when the platform has alternatives.
        if scheme_count > 1 && self.players[0].scheme == self.players[1].scheme {
            self.players[1].scheme = (self.players[0].scheme + 1) % scheme_count;
        }
        self.record_solo = self.record_solo.max(0);
        self.record_duo = self.record_duo.max(0);

        // Snap to the step, so a hand-edited file cannot leave the menu showing
        // a value its own controls could never produce.
        for v in [&mut self.music_volume, &mut self.sfx_volume] {
            *v = (*v).min(VOLUME_MAX);
            *v = ((*v + VOLUME_STEP / 2) / VOLUME_STEP) * VOLUME_STEP;
            *v = (*v).min(VOLUME_MAX);
        }
    }

    pub fn volume(&self, channel: VolumeChannel) -> u32 {
        match channel {
            VolumeChannel::Music => self.music_volume,
            VolumeChannel::Sfx => self.sfx_volume,
        }
    }

    /// Steps a volume by one notch. Clamps rather than wrapping: rolling from
    /// full to silence on one keypress is never what anyone wanted.
    pub fn adjust_volume(&mut self, channel: VolumeChannel, dir: i32) {
        let current = self.volume(channel) as i32;
        let stepped = (current + dir * VOLUME_STEP as i32).clamp(0, VOLUME_MAX as i32) as u32;
        if stepped == self.volume(channel) {
            return;
        }
        match channel {
            VolumeChannel::Music => self.music_volume = stepped,
            VolumeChannel::Sfx => self.sfx_volume = stepped,
        }
        self.dirty = true;
    }

    pub fn record(&self, player_count: usize) -> i64 {
        if player_count >= 2 {
            self.record_duo
        } else {
            self.record_solo
        }
    }

    pub fn set_record(&mut self, player_count: usize, value: i64) {
        if player_count >= 2 {
            self.record_duo = value;
        } else {
            self.record_solo = value;
        }
        self.dirty = true;
    }

    /// Steps a player's scheme or colour, skipping whatever the other player
    /// holds so the two can never collide.
    pub fn cycle(&mut self, player_index: usize, is_scheme: bool, dir: i32, scheme_count: usize) {
        let other = if player_index == 0 { 1 } else { 0 };
        let len = if is_scheme {
            scheme_count.max(1)
        } else {
            PLAYER_COLORS.len()
        } as i32;
        if len <= 1 {
            return;
        }

        let taken = if is_scheme {
            self.players[other].scheme
        } else {
            self.players[other].color_index
        } as i32;

        let mut index = if is_scheme {
            self.players[player_index].scheme
        } else {
            self.players[player_index].color_index
        } as i32;

        for _ in 0..len {
            index = (index + dir).rem_euclid(len);
            if index != taken {
                break;
            }
        }

        if is_scheme {
            self.players[player_index].scheme = index as usize;
        } else {
            self.players[player_index].color_index = index as usize;
        }
        self.dirty = true;
    }
}
