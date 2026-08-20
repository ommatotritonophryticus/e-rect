//! Enemies built by rolling dice instead of being picked off a list.
//!
//! The named variants are ten fixed points in a space the enemy struct could
//! always describe: nine of them differ only in which single trick they carry,
//! because size, speed and armour were combinable and behaviour was not. With
//! movement split from behaviour that space is open, and this is what walks it.
//!
//! Nothing here replaces the roster. A rolled enemy is a *tenth* source on top
//! of it, so the player still learns nine legible things and meets the strange
//! ones as occasions.

use crate::color::Rgb;
use crate::config::*;
use crate::entities::{Behaviors, Leap, Movement, Rng, Zombie};
use crate::geom::Viewport;

/// What an elite carries: twice what it takes to bring down an armoured enemy.
///
/// Armour stays at the armoured variant's own value, so this reads as "two of
/// those" rather than as an arbitrary number - 32 connecting ticks, about
/// eleven swings, against a wave-5 boss's thirteen.
pub const ELITE_HP: f32 = 255.0 * 2.0;

/// Paid per wave when one goes down.
///
/// It is not flagged as a boss - that flag also decides how the wall treats it,
/// and a flat 300 already stops the wall deleting it outright - so the payout
/// has to be carried separately. Eleven swings for six points would be an
/// insult.
pub const ELITE_REWARD_PER_WAVE: i64 = 50;

/// The one look every elite wears.
///
/// A rolled enemy has no signature colour by definition: colour is the only
/// language this game has for saying what something does, and a combination has
/// nothing to say in it. So it says something else instead - "this one is not
/// off the list" - and leaves the player to read the rest from behaviour: a
/// shade no ordinary enemy wears, and the name it is announced under.
pub const ELITE_COLOR: Rgb = Rgb::new(64.0, 64.0, 88.0);

/// Chance each behaviour is rolled in. Independent, so a plain elite - heavy
/// and nothing else - is a perfectly ordinary outcome.
const BEHAVIOUR_CHANCE: f32 = 0.45;

/// Chance it throws out young when struck. Rarer than the rest because it is
/// the one trait that answers a hit with more enemies rather than with a
/// different enemy.
const BROOD_CHANCE: f32 = 0.25;

/// The wave each trait becomes available, taken from where the fixed roster
/// introduces the enemy it belongs to.
///
/// A rolled enemy is read against the roster: the player recognises a hop or a
/// blink because they have met the enemy that does it. A trait handed out
/// before its owner has ever appeared is not a combination of known parts, it
/// is a surprise - which is how a wave-six heavy could turn up shooting,
/// breeding and laying hazards before any of the three had been seen once.
pub mod unlock {
    use crate::config::SHEDDER_MIN_WAVE;

    pub const RUN: i64 = 1; // the base enemy
    pub const FLY: i64 = 1; // the base flyer
    pub const HOP: i64 = 3; // the jumper
    pub const LEAP: i64 = 4; // the leaper

    pub const NORMAL: i64 = 1;
    pub const FLYER_SIZED: i64 = 1;
    pub const SMALL: i64 = 2; // the runt
    pub const LARGE: i64 = 5; // the first boss wave

    pub const BROOD: i64 = 7; // the splitter, the first enemy that makes more
    pub const BLINK: i64 = 8; // the blinker
    pub const SHOOT: i64 = 9; // the shooter
    pub const SHED: i64 = SHEDDER_MIN_WAVE;
}

/// Most young one blow can produce.
pub const BROOD_MAX: i32 = 3;

/// How an elite gets about. The state-carrying [`Movement`] is built from this;
/// a recipe only needs to name the choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Run,
    Hop,
    Leap,
    Fly,
}

impl MoveKind {
    pub const ALL: [MoveKind; 4] = [MoveKind::Run, MoveKind::Hop, MoveKind::Leap, MoveKind::Fly];

    pub fn label(self) -> &'static str {
        match self {
            MoveKind::Run => "RUNNER",
            MoveKind::Hop => "HOPPER",
            MoveKind::Leap => "LEAPER",
            MoveKind::Fly => "FLIER",
        }
    }
}

/// Independent of how it moves: a flier can be the size of a boss, and a runner
/// the size of a flyer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Large,
    Normal,
    Small,
    /// The footprint of an ordinary flyer - as wide as a player, half as tall.
    Flyer,
}

impl Size {
    pub const ALL: [Size; 4] = [Size::Large, Size::Normal, Size::Small, Size::Flyer];

    /// Multiplies the ordinary body, which is 5% of the view by 10%.
    pub fn scale(self) -> (f32, f32) {
        match self {
            // Exactly what a boss stands at: 10% by 15%.
            Size::Large => (2.0, 1.5),
            Size::Normal => (1.0, 1.0),
            Size::Small => (RUNT_SIZE_SCALE, RUNT_SIZE_SCALE),
            Size::Flyer => (1.0, 0.5),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Size::Large => "HUGE",
            Size::Normal => "",
            Size::Small => "SMALL",
            Size::Flyer => "SLIGHT",
        }
    }
}

/// One rolled enemy, before it is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub movement: MoveKind,
    pub size: Size,
    pub shoot: bool,
    pub blink: bool,
    pub shed: bool,
    /// Answers every blow it survives with young, built by rolling in turn.
    /// Theirs never carry it: a brood that broods is not a fight, it is a
    /// division that never stops.
    pub brood: bool,
}

/// One of `all` that `wave` has reached, or `fallback` if it has reached none.
///
/// The fallback is unreachable today - both axes have an entry open from the
/// first wave - but an axis is a list somebody will add to, and a later entry
/// should not be able to turn this into a panic.
fn pick<T: Copy + Unlockable>(all: &[T], wave: i64, fallback: T, rng: &mut Rng) -> T {
    let open = all.iter().filter(|t| wave >= t.min_wave()).count();
    let roll = rng.unit();
    if open == 0 {
        return fallback;
    }
    let want = ((roll * open as f32) as usize).min(open - 1);
    all.iter()
        .filter(|t| wave >= t.min_wave())
        .nth(want)
        .copied()
        .unwrap_or(fallback)
}

/// Anything with a wave it becomes available on.
pub trait Unlockable {
    fn min_wave(self) -> i64;
}

impl Unlockable for MoveKind {
    fn min_wave(self) -> i64 {
        match self {
            MoveKind::Run => unlock::RUN,
            MoveKind::Hop => unlock::HOP,
            MoveKind::Leap => unlock::LEAP,
            MoveKind::Fly => unlock::FLY,
        }
    }
}

impl Unlockable for Size {
    fn min_wave(self) -> i64 {
        match self {
            Size::Large => unlock::LARGE,
            Size::Normal => unlock::NORMAL,
            Size::Small => unlock::SMALL,
            Size::Flyer => unlock::FLYER_SIZED,
        }
    }
}

impl Recipe {
    /// Rolls one.
    ///
    /// There is no compatibility table, and that is a result rather than an
    /// omission: the axes stopped conflicting the moment movement became a
    /// choice of one. Two ways of moving cannot both be picked, and no two
    /// behaviours write the same field.
    /// Draws a combination out of what `wave` has already introduced.
    ///
    /// Every draw happens whatever the wave allows, so a trait being shut does
    /// not shift the stream the rest of the run draws from - the same reason
    /// the wave kind and rule are both rolled even when one is pinned.
    pub fn roll(wave: i64, rng: &mut Rng) -> Self {
        let movement = pick(&MoveKind::ALL, wave, MoveKind::Run, rng);
        let size = pick(&Size::ALL, wave, Size::Normal, rng);
        Self {
            movement,
            size,
            shoot: rng.unit() < BEHAVIOUR_CHANCE && wave >= unlock::SHOOT,
            blink: rng.unit() < BEHAVIOUR_CHANCE && wave >= unlock::BLINK,
            shed: rng.unit() < BEHAVIOUR_CHANCE && wave >= unlock::SHED,
            brood: rng.unit() < BROOD_CHANCE && wave >= unlock::BROOD,
        }
    }

    /// What to call it, for anything that wants to say so on screen.
    ///
    /// Movement first because it is what the player reads first - the thing is
    /// across the field before its tricks show.
    pub fn label(&self) -> alloc::string::String {
        use alloc::string::ToString;
        let mut out = alloc::string::String::new();
        if !self.size.label().is_empty() {
            out.push_str(self.size.label());
            out.push(' ');
        }
        out.push_str(self.movement.label());
        for (on, word) in [
            (self.shoot, "GUN"),
            (self.blink, "BLINK"),
            (self.shed, "TRAP"),
            (self.brood, "BROOD"),
        ] {
            if on {
                out.push(' ');
                out.push_str(&word.to_string());
            }
        }
        out
    }

    /// Builds the enemy this recipe describes, at the edge of the view.
    pub fn build(&self, v: &Viewport, wave: i64, timer: i64, rng: &mut Rng) -> Zombie {
        let mut z = Zombie::from_edge(v, rng, ELITE_COLOR);

        let (sw, sh) = self.size.scale();
        z.body.w *= sw;
        z.body.h *= sh;
        // Standing on the floor whatever its height, so a short one does not
        // hover and a tall one does not sink.
        z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;

        z.hp = ELITE_HP;
        z.hpmax = ELITE_HP;
        z.armor = ARMORED_ARMOR;
        z.elite = true;
        z.reward = ELITE_REWARD_PER_WAVE * wave.max(1);

        z.movement = match self.movement {
            MoveKind::Run => Movement::Run,
            MoveKind::Hop => Movement::Hop {
                cooldown: rng.range(0, JUMPER_JUMP_EVERY),
            },
            MoveKind::Leap => Movement::Leap(Leap {
                crouch: LEAPER_CROUCH_TICKS,
                airborne: false,
            }),
            // The phase decides where in the arc it enters, so two fliers
            // spawned together do not fly in lockstep.
            MoveKind::Fly => Movement::Fly {
                offset: timer - rng.range(0, 314) as i64,
            },
        };

        z.behaviors = Behaviors {
            shoot: self
                .shoot
                .then(|| rng.range(0, SHOOTER_FIRE_EVERY)),
            blink: self.blink,
            shed: self.shed,
        };
        if self.shed {
            z.max_husks = SHEDDER_HUSKS;
        }
        z.broods = self.brood;
        z
    }

    /// The same combination at ordinary strength.
    ///
    /// What a brooding elite throws out. They inherit the *shape* of a rolled
    /// enemy - any movement, any size, any tricks - but none of its weight:
    /// eleven swings each, several per blow, would not be a fight. And never
    /// the brooding itself, whatever the roll said.
    pub fn build_minion(&self, v: &Viewport, timer: i64, rng: &mut Rng) -> Zombie {
        let mut z = self.build(v, 1, timer, rng);
        z.hp = 255.0;
        z.hpmax = 255.0;
        z.armor = 1.0;
        z.reward = 6;
        // Not marked: the halo means "one of the rolled heavies", and a minion
        // is not one. Its colour still says whose it is.
        z.elite = false;
        z.broods = false;
        z
    }
}
