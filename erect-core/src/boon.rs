//! Standing upgrades that are not weapons.
//!
//! An attack is exclusive - carrying one means not carrying the rest, and
//! swapping costs whatever level the old one had reached. A boon is nothing
//! like that: it is kept, it costs nothing to take, and it never comes up
//! again. The two sit in the same offer anyway, which means a boon on the table
//! is always the safe pick. What keeps that from swallowing the weapon roster
//! is that the pool drains: three boons exist, and once they are held the offer
//! has nothing left to give but weapons.

use crate::color::Rgb;
use crate::config::TICKS_PER_SEC;

/// Ticks between one absorbed hit and the next.
///
/// A second is long against a single enemy and short against a crowd, which is
/// the right way round: it caps how fast a swarm can chew through the player
/// without doing much for someone taking one hit at a time.
pub const SHIELD_COOLDOWN_TICKS: i64 = TICKS_PER_SEC as i64;

/// What the wall does to everything on the field when it goes up, beyond
/// standing there.
///
/// The two are one setting rather than two switches, because a wall cannot pull
/// and push at once. Taking one replaces the other, and because the offer never
/// shows what is already held, the swap is exactly what gets offered next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WallMod {
    #[default]
    Plain,
    /// Black. Drags every enemy toward the player who raised it.
    Pull,
    /// Grey. Throws every enemy away from them.
    Push,
}

impl WallMod {
    /// What the wall is drawn in. A flat slab and nothing else: no outline, so
    /// the colour is the whole of the signal.
    pub fn color(self) -> Rgb {
        match self {
            WallMod::Plain => Rgb::new(255.0, 255.0, 255.0),
            WallMod::Pull => Rgb::new(8.0, 8.0, 8.0),
            WallMod::Push => Rgb::new(140.0, 140.0, 140.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boon {
    /// One jump in the air, back after the next landing.
    DoubleJump,
    /// The dash keeps only its energy price.
    DashFree,
    /// Absorbs one touch, then waits [`SHIELD_COOLDOWN_TICKS`].
    Shield,
    /// The wall turns black and gathers the field in.
    WallPull,
    /// The wall turns grey and clears the field out.
    WallPush,
}

impl Boon {
    pub const ALL: [Boon; 5] = [
        Boon::DoubleJump,
        Boon::DashFree,
        Boon::Shield,
        Boon::WallPull,
        Boon::WallPush,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Boon::DoubleJump => "DOUBLE JUMP",
            Boon::DashFree => "FREE DASH",
            Boon::Shield => "SHIELD",
            Boon::WallPull => "BLACK WALL",
            Boon::WallPush => "GREY WALL",
        }
    }
}

/// Which boons a run is carrying. Held per player, but taken together: the
/// offer is shared, so what one takes everyone gets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Boons {
    pub double_jump: bool,
    pub dash_free: bool,
    pub shield: bool,
    pub wall: WallMod,
}

impl Boons {
    pub fn has(&self, boon: Boon) -> bool {
        match boon {
            Boon::DoubleJump => self.double_jump,
            Boon::DashFree => self.dash_free,
            Boon::Shield => self.shield,
            Boon::WallPull => self.wall == WallMod::Pull,
            Boon::WallPush => self.wall == WallMod::Push,
        }
    }

    pub fn take(&mut self, boon: Boon) {
        match boon {
            Boon::DoubleJump => self.double_jump = true,
            Boon::DashFree => self.dash_free = true,
            Boon::Shield => self.shield = true,
            // Assignment, not accumulation: the other one stops being true by
            // the same stroke, which is what makes the pair a choice.
            Boon::WallPull => self.wall = WallMod::Pull,
            Boon::WallPush => self.wall = WallMod::Push,
        }
    }
}
