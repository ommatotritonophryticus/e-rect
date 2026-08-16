//! Title and settings menus.
//!
//! Rows are laid out from `top_pct` deterministically so the renderer and the
//! game agree on geometry without passing anything between them.

use alloc::string::String;

use crate::config::MENU_ROW_H_PCT;
use crate::settings::VolumeChannel;

/// What activating a row should do. Rows that carry a value are adjusted with
/// left/right instead of being confirmed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
    StartRun(usize),
    OpenSettings,
    Back,
    AdjustScheme(usize),
    AdjustColor(usize),
    AdjustVolume(VolumeChannel),
    /// Close the pause menu and carry on.
    Resume,
    /// Cut the lull short and bring the next wave on now.
    StartWave,
    /// Leaving a run throws away its progress, so it asks first.
    AskAbandon,
    AbandonRun,
    KeepPlaying,
    /// Leaving the game is the platform's business, so the core only records
    /// that it was asked for.
    Quit,

    /* developer menu */
    AdjustDevWave,
    AdjustDevScore,
    AdjustDevKind,
    AdjustDevRule,
    AdjustDevPlayers,
    AdjustDevAttack,
    AdjustDevAttackLevel,
    /// Begin a run on the parameters set above.
    StartDevRun,
}

pub struct MenuRow {
    pub label: String,
    pub action: MenuAction,
    /// Colour swatch drawn to the left of the label, for the colour rows.
    pub swatch: Option<usize>,
}

impl MenuRow {
    pub fn is_adjustable(&self) -> bool {
        matches!(
            self.action,
            MenuAction::AdjustScheme(_)
                | MenuAction::AdjustColor(_)
                | MenuAction::AdjustVolume(_)
                | MenuAction::AdjustDevWave
                | MenuAction::AdjustDevScore
                | MenuAction::AdjustDevKind
                | MenuAction::AdjustDevRule
                | MenuAction::AdjustDevPlayers
                | MenuAction::AdjustDevAttack
                | MenuAction::AdjustDevAttackLevel
        )
    }
}

pub struct Menu {
    pub top_pct: f32,
    pub index: usize,
    /// Vertical pitch between rows. Its own field rather than the shared
    /// constant because a long menu has to tighten up to fit: nine rows at the
    /// usual pitch run off the bottom of a 480x272 screen.
    pub row_h_pct: f32,
}

impl Menu {
    pub fn new(top_pct: f32) -> Self {
        Self { top_pct, index: 0, row_h_pct: MENU_ROW_H_PCT }
    }

    /// A menu with more rows than the usual pitch leaves room for.
    pub fn tight(top_pct: f32, row_h_pct: f32) -> Self {
        Self { top_pct, index: 0, row_h_pct }
    }

    pub fn row_y_pct(&self, index: usize) -> f32 {
        self.top_pct + index as f32 * self.row_h_pct
    }

    pub fn move_by(&mut self, delta: i32, row_count: usize) {
        if row_count == 0 {
            return;
        }
        let n = row_count as i32;
        self.index = ((self.index as i32 + delta).rem_euclid(n)) as usize;
    }
}
