//! Starting parameters for a run launched from the developer menu.
//!
//! Reaching wave 20 the honest way takes the better part of half an hour, which
//! makes anything that only goes wrong late effectively untestable by hand.
//! This is the shortcut, and it is deliberately a separate entry point rather
//! than a flag on the ordinary one: nothing here can be reached by a player who
//! did not go looking for it.

use crate::attack::{AttackKind, MAX_LEVEL};
use crate::boon::{Boons, WallMod};
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

/// Every attack the menu can hand out, with the one the game starts on first.
/// `AttackKind::ALL` is the upgrade roster and deliberately excludes it.
/// Every wall the menu can hand out, plain first.
const WALLS: [WallMod; 3] = [WallMod::Plain, WallMod::Pull, WallMod::Push];

const ATTACKS: [AttackKind; 9] = [
    AttackKind::Basic,
    AttackKind::Hammer,
    AttackKind::Piercing,
    AttackKind::Lunge,
    AttackKind::SingleHit,
    AttackKind::Thin,
    AttackKind::Tall,
    AttackKind::Bullet,
    AttackKind::Frozen,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DevSetup {
    pub wave: i64,
    /// Handed to the first player, so the team total is the number on screen.
    pub score: i64,
    pub kind: Option<WaveKind>,
    pub rule: Option<WaveRule>,
    pub players: usize,
    pub attack: AttackKind,
    pub attack_level: u8,
    /// Standing upgrades to start the run holding. The offer normally hands
    /// these out one at a time over tens of thousands of points, which is the
    /// whole reason to be able to set them here.
    pub boons: Boons,
}

impl Default for DevSetup {
    fn default() -> Self {
        Self {
            wave: 1,
            score: 0,
            kind: None,
            rule: None,
            players: 1,
            attack: AttackKind::Basic,
            attack_level: 1,
            boons: Boons::default(),
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

    pub fn cycle_attack(&mut self, dir: i32) {
        let at = ATTACKS.iter().position(|a| *a == self.attack).unwrap_or(0);
        self.attack = ATTACKS[step(at, dir, ATTACKS.len())];
    }

    pub fn adjust_attack_level(&mut self, dir: i32) {
        let level = (self.attack_level as i32 + dir).clamp(1, MAX_LEVEL as i32);
        self.attack_level = level as u8;
    }

    pub fn toggle_double_jump(&mut self) {
        self.boons.double_jump = !self.boons.double_jump;
    }

    pub fn toggle_dash_free(&mut self) {
        self.boons.dash_free = !self.boons.dash_free;
    }

    pub fn toggle_shield(&mut self) {
        self.boons.shield = !self.boons.shield;
    }

    pub fn cycle_wall(&mut self, dir: i32) {
        let at = WALLS.iter().position(|w| *w == self.boons.wall).unwrap_or(0);
        self.boons.wall = WALLS[step(at, dir, WALLS.len())];
    }

    /// True when the run would start holding everything there is.
    pub fn all_boons(&self) -> bool {
        self.boons.double_jump
            && self.boons.dash_free
            && self.boons.shield
            && self.boons.wall != WallMod::Plain
    }

    /// Every boon at once, or none. What "set all the upgrades" means when it
    /// is one press: the wall goes to the first modified one rather than back
    /// to plain, because plain is what "off" already looks like.
    pub fn toggle_all_boons(&mut self) {
        self.boons = if self.all_boons() {
            Boons::default()
        } else {
            Boons {
                double_jump: true,
                dash_free: true,
                shield: true,
                wall: WallMod::Pull,
            }
        };
    }

    pub fn wall_label(&self) -> &'static str {
        match self.boons.wall {
            WallMod::Plain => "PLAIN",
            WallMod::Pull => "BLACK",
            WallMod::Push => "GREY",
        }
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
