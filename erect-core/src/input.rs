//! What the player is asking for this tick, as plain data.
//!
//! Frontends fill this in from whatever they have - macroquad key polling and
//! gilrs on a desktop, sceCtrl on a PSP. The core never touches a device.

use crate::config::MAX_PLAYERS;

/// Held state for movement, edge-triggered for one-shot actions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerIntent {
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub slam: bool,
    pub attack: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuIntent {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub confirm: bool,
    pub back: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputFrame {
    pub players: [PlayerIntent; MAX_PLAYERS],
    pub menu: MenuIntent,
    pub pause: bool,
    /// Drives the "(OFF)" marker on the settings screen.
    pub pads_connected: [bool; MAX_PLAYERS],
}

impl InputFrame {
    /// One-shot intents must fire on exactly one simulation tick, even when a
    /// rendered frame ran several of them.
    pub fn clear_edges(&mut self) {
        for p in self.players.iter_mut() {
            p.jump = false;
            p.slam = false;
            p.attack = false;
        }
        self.menu = MenuIntent::default();
        self.pause = false;
    }
}
