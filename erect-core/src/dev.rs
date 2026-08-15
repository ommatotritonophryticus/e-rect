//! Starting parameters for a run launched from the developer menu.
//!
//! Reaching wave 20 the honest way takes the better part of half an hour, which
//! makes anything that only goes wrong late effectively untestable by hand.
//! This is the shortcut, and it is deliberately a separate entry point rather
//! than a flag on the ordinary one: nothing here can be reached by a player who
//! did not go looking for it.

use crate::waves::{WaveKind, WaveRule};

/// Score moves in steps this size. A run worth of score is thousands of points,
/// so single units would be hundreds of presses.
pub const DEV_SCORE_STEP: i64 = 500;
/// Far past anything reachable, and far short of anything that overflows.
pub const DEV_MAX_WAVE: i64 = 99;
pub const DEV_MAX_SCORE: i64 = 500_000;

/// `None` on either modifier means "leave it to roll", exactly as a real run
/// does; anything else pins it for every wave of the run.
const KINDS: [Option<WaveKind>; 6] = [
    None,
    Some(WaveKind::Mixed),
    Some(WaveKind::GroundOnly),
    Some(WaveKind::FlyersOnly),
    Some(WaveKind::BasicOnly),
    Some(WaveKind::JumpersOnly),
];

const RULES: [Option<WaveRule>; 6] = [
    None,
    Some(WaveRule::Normal),
    Some(WaveRule::StaticCamera),
    Some(WaveRule::NoJumps),
    Some(WaveRule::NoWall),
    Some(WaveRule::Hidden),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DevSetup {
    pub wave: i64,
    /// Handed to the first player, so the team total is the number on screen.
    pub score: i64,
    pub kind: Option<WaveKind>,
    pub rule: Option<WaveRule>,
    pub players: usize,
}

impl Default for DevSetup {
    fn default() -> Self {
        Self {
            wave: 1,
            score: 0,
            kind: None,
            rule: None,
            players: 1,
        }
    }
}

/// Steps `index` by `dir` and wraps, for the fixed lists above.
fn step(index: usize, dir: i32, len: usize) -> usize {
    ((index as i32 + dir).rem_euclid(len as i32)) as usize
}

impl DevSetup {
    pub fn adjust_wave(&mut self, dir: i32) {
        self.wave = (self.wave + dir as i64).clamp(1, DEV_MAX_WAVE);
    }

    pub fn adjust_score(&mut self, dir: i32) {
        self.score = (self.score + dir as i64 * DEV_SCORE_STEP).clamp(0, DEV_MAX_SCORE);
    }

    pub fn adjust_players(&mut self, dir: i32, max_players: usize) {
        let max = max_players.max(1) as i32;
        self.players = (self.players as i32 + dir).clamp(1, max) as usize;
    }

    pub fn cycle_kind(&mut self, dir: i32) {
        let at = KINDS.iter().position(|k| *k == self.kind).unwrap_or(0);
        self.kind = KINDS[step(at, dir, KINDS.len())];
    }

    pub fn cycle_rule(&mut self, dir: i32) {
        let at = RULES.iter().position(|r| *r == self.rule).unwrap_or(0);
        self.rule = RULES[step(at, dir, RULES.len())];
    }

    /// `WaveKind::label` returns nothing for the ordinary mix, because a real
    /// run has nothing to announce there. A menu does.
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            None => "ANY",
            Some(kind) => kind.label().unwrap_or("MIXED"),
        }
    }

    pub fn rule_label(&self) -> &'static str {
        match self.rule {
            None => "ANY",
            Some(rule) => rule.label().unwrap_or("NORMAL"),
        }
    }
}
