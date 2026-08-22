//! Wave pacing and the weighted spawn tables.
//!
//! The browser version drove spawning from chained `setTimeout` calls running
//! independently of the render loop. Here it is a tick-driven state machine, so
//! everything advances on the same fixed clock as the simulation.

use crate::entities::Rng;

/// Which enemy variant to build, once the caller has decided "spawn a ground
/// enemy" or "spawn a flyer". Each entry unlocks at `min_wave` and is chosen by
/// weight among those currently unlocked, so new variants phase in gradually
/// instead of replacing the base roster.
pub struct SpawnEntry {
    pub weight: u32,
    pub min_wave: i64,
    pub kind: GroundKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroundKind {
    Base,
    Runt,
    Jumper,
    Leaper,
    Armored,
    Frenzied,
    Splitter,
    Blinker,
    Shooter,
    /// Blinks out of reach when hurt and leaves a standing hazard where it was.
    Shedder,
}

/// Which boss to put out. From [`BOSS_RAMP_FIRST_WAVE`] each one in a wave is
/// drawn separately, so a late boss wave is a mixed set rather than a row of
/// the same thing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BossKind {
    /// The white slab the game has always had.
    Ground,
    Flying,
    /// Blinks away when hurt and leaves a standing hazard.
    Shedder,
    /// A rolled combination at boss health, which makes its own crowd.
    Rolled,
}

impl BossKind {
    pub const ALL: [BossKind; 4] = [
        BossKind::Ground,
        BossKind::Flying,
        BossKind::Shedder,
        BossKind::Rolled,
    ];

    fn roll(rng: &mut Rng) -> Self {
        Self::ALL[rng.range(0, Self::ALL.len() as i32 - 1) as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlyerKind {
    Base,
    Teleporter,
}

pub const GROUND_SPAWN_TABLE: [SpawnEntry; 10] = [
    SpawnEntry {
        weight: 6,
        min_wave: 1,
        kind: GroundKind::Base,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 2,
        kind: GroundKind::Runt,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 3,
        kind: GroundKind::Jumper,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 4,
        kind: GroundKind::Leaper,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 5,
        kind: GroundKind::Armored,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 6,
        kind: GroundKind::Frenzied,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 7,
        kind: GroundKind::Splitter,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 8,
        kind: GroundKind::Blinker,
    },
    SpawnEntry {
        weight: 2,
        min_wave: 9,
        kind: GroundKind::Shooter,
    },
    SpawnEntry {
        weight: 2,
        min_wave: crate::config::SHEDDER_MIN_WAVE,
        kind: GroundKind::Shedder,
    },
];

pub struct FlyerSpawnEntry {
    pub weight: u32,
    pub min_wave: i64,
    pub kind: FlyerKind,
}

pub const FLYER_SPAWN_TABLE: [FlyerSpawnEntry; 2] = [
    FlyerSpawnEntry {
        weight: 3,
        min_wave: 1,
        kind: FlyerKind::Base,
    },
    FlyerSpawnEntry {
        weight: 2,
        min_wave: 4,
        kind: FlyerKind::Teleporter,
    },
];

pub fn pick_ground(wave: i64, rng: &mut Rng) -> GroundKind {
    let total: u32 = GROUND_SPAWN_TABLE
        .iter()
        .filter(|e| wave >= e.min_wave)
        .map(|e| e.weight)
        .sum();
    let mut roll = rng.unit() * total as f32;
    for entry in GROUND_SPAWN_TABLE.iter().filter(|e| wave >= e.min_wave) {
        roll -= entry.weight as f32;
        if roll < 0.0 {
            return entry.kind;
        }
    }
    GroundKind::Base
}

pub fn pick_flyer(wave: i64, rng: &mut Rng) -> FlyerKind {
    let total: u32 = FLYER_SPAWN_TABLE
        .iter()
        .filter(|e| wave >= e.min_wave)
        .map(|e| e.weight)
        .sum();
    let mut roll = rng.unit() * total as f32;
    for entry in FLYER_SPAWN_TABLE.iter().filter(|e| wave >= e.min_wave) {
        roll -= entry.weight as f32;
        if roll < 0.0 {
            return entry.kind;
        }
    }
    FlyerKind::Base
}

/// Occasionally a wave is restricted to one kind of enemy, which changes how it
/// has to be fought: an all-flyer wave cannot be meleed the usual way, an
/// all-jumper wave never leaves the ground clear.
///
/// A restricted wave ignores the usual `min_wave` gating - that is the point of
/// it. A jumpers-only wave can turn up before jumpers would normally appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveKind {
    /// The ordinary weighted mix.
    Mixed,
    /// Ground enemies only, any variant.
    GroundOnly,
    FlyersOnly,
    /// Nothing but plain zombies.
    BasicOnly,
    JumpersOnly,
}

/// How a wave plays, as opposed to who turns up in it.
///
/// A separate axis from [`WaveKind`] on purpose: one enum holding both would
/// make the two mutually exclusive, and "jumpers only, and you cannot jump" is
/// exactly the sort of pairing worth having. At most one of each per wave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveRule {
    Normal,
    /// The view stops where the wave began and the player is walled in. Ground
    /// enemies walk to them and flyers are held in view anyway, so only the
    /// player needs the walls.
    StaticCamera,
    /// No jumping. Down puts the wall up from standing, and the attack box is
    /// twice as long to make up for the lost mobility.
    NoJumps,
    /// No wall. The slam button instead buys invulnerability in mid-air, and
    /// landing heals a quarter of full health.
    NoWall,
    /// Nothing visible but the player, its attacks, blasts and score popups.
    /// Almost harmless, since fighting blind otherwise is not a fight.
    Hidden,
}

impl WaveRule {
    const CHANCE: f32 = 0.25;

    pub fn label(self) -> Option<&'static str> {
        match self {
            WaveRule::Normal => None,
            WaveRule::StaticCamera => Some("HOLD THE LINE"),
            WaveRule::NoJumps => Some("GROUNDED"),
            WaveRule::NoWall => Some("NO WALL"),
            WaveRule::Hidden => Some("BLIND"),
        }
    }

    /// How much longer the attack box is on this wave.
    pub fn gun_reach(self) -> f32 {
        match self {
            WaveRule::NoJumps => 2.0,
            _ => 1.0,
        }
    }

    /// What enemy damage is multiplied by.
    pub fn damage_scale(self) -> f32 {
        match self {
            WaveRule::Hidden => 0.01,
            _ => 1.0,
        }
    }

    fn roll(wave: i64, rng: &mut Rng) -> Self {
        // The first wave is always plain, the same as for enemy kinds: the
        // opening should teach the game, not a variant of it.
        if wave <= 1 || rng.unit() >= Self::CHANCE {
            return WaveRule::Normal;
        }
        // `range` is inclusive at both ends, so four variants need 0..=3. With
        // 0..=4 the catch-all swallowed two values and the last variant came up
        // twice as often as the rest.
        match rng.range(0, 3) {
            0 => WaveRule::StaticCamera,
            1 => WaveRule::NoJumps,
            2 => WaveRule::NoWall,
            _ => WaveRule::Hidden,
        }
    }
}

impl WaveKind {
    /// One in four waves is restricted.
    const SPECIAL_CHANCE: f32 = 0.25;

    /// Shown during the countdown so the player can see it coming.
    pub fn label(self) -> Option<&'static str> {
        match self {
            WaveKind::Mixed => None,
            WaveKind::GroundOnly => Some("GROUND ONLY"),
            WaveKind::FlyersOnly => Some("FLYERS ONLY"),
            WaveKind::BasicOnly => Some("HORDE"),
            WaveKind::JumpersOnly => Some("JUMPERS"),
        }
    }

    /// Rolls the kind for an upcoming wave.
    ///
    /// Wave 1 and boss waves stay mixed: the first wave is the introduction,
    /// and a boss wave already is the special one.
    pub fn roll(wave: i64, rng: &mut Rng) -> Self {
        if wave <= 1 || wave % 5 == 0 || rng.unit() >= Self::SPECIAL_CHANCE {
            return WaveKind::Mixed;
        }
        // Inclusive range: four variants need 0..=3, not 0..=4.
        match rng.range(0, 3) {
            0 => WaveKind::GroundOnly,
            1 => WaveKind::FlyersOnly,
            2 => WaveKind::BasicOnly,
            _ => WaveKind::JumpersOnly,
        }
    }
}

/// What the wave manager wants the game to do this tick.
// Not `Copy`: a late boss wave carries a list of what it drew.
#[derive(Clone, Debug, PartialEq)]
pub enum WaveAction {
    Idle,
    SpawnGround(GroundKind),
    SpawnFlyer(FlyerKind),
    SpawnBosses(i64),
    /// Wave 10's single flying boss, in place of that wave's ground bosses.
    SpawnFlyingBoss,
    /// Wave 15's alternative: one shedder boss instead of that wave's three
    /// ground bosses. Rolled, so the wave is not the same fight every run.
    SpawnShedderBoss,
    /// A late boss wave's mixed set, each drawn separately.
    SpawnBossGroup(alloc::vec::Vec<BossKind>),
    /// Wave 20's whole content: one rolled boss and nothing else at all.
    SpawnRolledBoss,
    /// This many rolled heavies at once, from [`ELITE_FIRST_WAVE`] on. What
    /// they turn out to be is the caller's business - the manager only decides
    /// how many and when.
    SpawnElite(usize),
    ClearWave,
}

pub struct WaveManager {
    /// What kind of wave is running. Rolled when a wave begins.
    pub kind: WaveKind,
    pub rule: WaveRule,
    /// Pinned by the developer menu. `None` leaves the roll alone, which is
    /// what every ordinary run uses.
    pub forced_kind: Option<WaveKind>,
    pub forced_rule: Option<WaveRule>,
    boss_due: bool,
    /// Rolled heavies this wave still owes, and how many come out together.
    ///
    /// A count rather than a flag: past [`ELITE_RAMP_FIRST_WAVE`] a wave is
    /// made of them, and they have to be spread through it rather than all
    /// turning up at one mark.
    elites_left: usize,
    elite_group: usize,
    /// -1 when no countdown is running, otherwise the number still displayed.
    pub countdown: i32,
    countdown_timer: i32,
    spawn_timer: i32,
}

impl Default for WaveManager {
    fn default() -> Self {
        Self {
            kind: WaveKind::Mixed,
            rule: WaveRule::Normal,
            forced_kind: None,
            forced_rule: None,
            boss_due: true,
            elites_left: 1,
            elite_group: 1,
            countdown: -1,
            countdown_timer: 0,
            spawn_timer: 0,
        }
    }
}

impl WaveManager {
    /// Back to a fresh run. The pinned modifiers survive on purpose: they are
    /// set before the run starts and are meant to hold for all of it.
    pub fn reset(&mut self) {
        let (kind, rule) = (self.forced_kind, self.forced_rule);
        *self = Self::default();
        self.forced_kind = kind;
        self.forced_rule = rule;
    }

    /// Ends the lull immediately; the next wave starts spawning at once.
    pub fn skip_countdown(&mut self) {
        self.countdown = -1;
        self.countdown_timer = 0;
    }

    /// True while the game is between waves.
    pub fn between_waves(&self) -> bool {
        self.countdown >= 0
    }

    pub fn begin_countdown(&mut self, seconds: i32) {
        self.countdown = seconds;
        self.countdown_timer = crate::config::TICKS_PER_SEC as i32;
    }

    /// Decides what the upcoming wave will be made of.
    pub fn begin_wave(&mut self, wave: i64, rng: &mut Rng) {
        // Both rolls happen either way, pinned or not, so that pinning one does
        // not shift the stream the rest of the run draws from.
        let (kind, rule) = (WaveKind::roll(wave, rng), WaveRule::roll(wave, rng));
        self.kind = self.forced_kind.unwrap_or(kind);
        self.rule = self.forced_rule.unwrap_or(rule);
        self.elites_left = crate::config::elites_in_wave(wave);
        self.elite_group = crate::config::elite_group_size(wave);
    }

    /// The spawn count the next heavy is due at.
    ///
    /// The arrivals are spread evenly across the wave rather than counted off
    /// from the start: a wave owing seven of them puts one at every seventh of
    /// its budget. The first is a whole interval in, so the wave always opens
    /// as an ordinary one - which is the same reason a single heavy waited for
    /// a quarter of the budget before this ramp existed.
    fn next_elite_at(&self, wave: i64) -> i64 {
        let budget = crate::config::wave_budget(wave);
        let total = crate::config::elites_in_wave(wave);
        let group = crate::config::elite_group_size(wave).max(1);
        // Arrivals, not heavies: a pair is one arrival and takes one slot.
        let arrivals = total.div_ceil(group).max(1) as i64;
        if arrivals == 1 {
            // Nothing to space out. It still waits, for the same reason one
            // ever did.
            return budget / crate::config::ELITE_ENTRY_FRACTION;
        }
        let done = (total - self.elites_left).div_ceil(group) as i64;
        (budget * (done + 1)) / (arrivals + 1)
    }

    /// Advances pacing by one tick and reports what should happen.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        wave: i64,
        spawn_count: i64,
        live_enemies: usize,
        rng: &mut Rng,
    ) -> WaveAction {
        if self.countdown >= 0 {
            self.countdown_timer -= 1;
            if self.countdown_timer <= 0 {
                self.countdown -= 1;
                self.countdown_timer = crate::config::TICKS_PER_SEC as i32;
            }
            return WaveAction::Idle;
        }

        if self.spawn_timer > 0 {
            self.spawn_timer -= 1;
            return WaveAction::Idle;
        }

        // The wave's heavies, spread through it. Boss waves are skipped: three
        // bosses and a rolled heavy in one wave is two headlines reading over
        // each other.
        if self.elites_left > 0
            && wave >= crate::config::ELITE_FIRST_WAVE
            && wave % 5 != 0
            && spawn_count >= self.next_elite_at(wave)
        {
            let group = self.elite_group.min(self.elites_left);
            self.elites_left -= group;
            self.spawn_timer = rng.range(6, 60);
            return WaveAction::SpawnElite(group);
        }

        // A wave that spawns nothing ordinary has spent its budget the moment
        // its boss is out; there is no count left to work through. Without this
        // the wave would sit forever a hundred and fifty short of a budget it
        // was never going to spend.
        if wave == crate::config::ROLLED_BOSS_WAVE && !self.boss_due {
            return if live_enemies == 0 {
                WaveAction::ClearWave
            } else {
                self.spawn_timer = 18;
                WaveAction::Idle
            };
        }

        if spawn_count < crate::config::wave_budget(wave) {
            // Boss waves drop their whole group in one go, once per wave.
            if wave % 5 == 0 {
                if self.boss_due {
                    self.boss_due = false;
                    self.spawn_timer = rng.range(6, 60); // 100-1000 ms at 60 Hz
                    // One wave belongs to the flying boss, and it comes alone;
                    // every other boss wave keeps its ground bosses.
                    return if wave >= crate::config::BOSS_RAMP_FIRST_WAVE {
                        // Past the ramp a boss wave is a count and a handful of
                        // draws rather than a set piece.
                        let n = crate::config::bosses_in_wave(wave);
                        WaveAction::SpawnBossGroup(
                            (0..n).map(|_| BossKind::roll(rng)).collect(),
                        )
                    } else if wave == crate::config::ROLLED_BOSS_WAVE {
                        WaveAction::SpawnRolledBoss
                    } else if wave == crate::config::FLYING_BOSS_WAVE {
                        WaveAction::SpawnFlyingBoss
                    } else if wave == crate::config::SHEDDER_BOSS_WAVE && rng.flip() {
                        // A coin flip rather than a fixed owner: wave 10 always
                        // belongs to the flying boss, but this wave is worth
                        // being two different fights.
                        WaveAction::SpawnShedderBoss
                    } else {
                        WaveAction::SpawnBosses(wave / 5)
                    };
                }
            } else {
                self.boss_due = true;
            }

            self.spawn_timer = rng.range(6, 60);

            if live_enemies < crate::config::max_concurrent_enemies(wave) {
                return match self.kind {
                    // A restricted wave spawns on every opportunity; the coin
                    // flips below are what make a mixed wave feel sparse, and
                    // applying them here would make special waves feel empty.
                    WaveKind::GroundOnly => WaveAction::SpawnGround(pick_ground(wave, rng)),
                    WaveKind::FlyersOnly => WaveAction::SpawnFlyer(pick_flyer(wave, rng)),
                    WaveKind::BasicOnly => WaveAction::SpawnGround(GroundKind::Base),
                    WaveKind::JumpersOnly => WaveAction::SpawnGround(if rng.flip() {
                        GroundKind::Jumper
                    } else {
                        GroundKind::Leaper
                    }),
                    WaveKind::Mixed => {
                        if rng.flip() {
                            WaveAction::SpawnGround(pick_ground(wave, rng))
                        } else if rng.flip() {
                            WaveAction::SpawnFlyer(pick_flyer(wave, rng))
                        } else {
                            WaveAction::Idle
                        }
                    }
                };
            }
            return WaveAction::Idle;
        }

        if live_enemies == 0 {
            return WaveAction::ClearWave;
        }

        // Budget spent but stragglers remain: check back shortly.
        self.spawn_timer = 18;
        WaveAction::Idle
    }
}
