//! The choice between waves.
//!
//! Three options stand in the lull as things to be hit. That is deliberate:
//! the player already knows how to attack, so nothing new has to be taught and
//! no button has to be borrowed from the fight. Walking up and swinging is the
//! whole interface.
//!
//! Both players share one offer and either can take it. The trigger is the team
//! total, which is also what drives the chase speed - so upgrades and difficulty
//! run off the same clock.

use crate::attack::{AttackKind, MAX_LEVEL};
use crate::entities::Rng;
use crate::geom::{Body, Viewport};

/// Team score between offers.
pub const OFFER_SCORE_STEP: i64 = 4000;

/// How many stand at once.
pub const OFFER_CHOICES: usize = 3;

/// Ticks the options stand there inert before a hit counts as taking one.
///
/// A wave ends while the player is still swinging at what is left of it, and
/// the options appear inside that swing. Without a pause the choice is made by
/// whichever one happened to be nearest when the last enemy died - which is not
/// a choice at all.
pub const OFFER_ARM_TICKS: i64 = 90;

/// Where they stand, as a percentage of the view. Spread wide enough that
/// reaching one is a decision rather than an accident of where you stopped.
const OFFER_X_PCT: [f32; OFFER_CHOICES] = [25.0, 50.0, 75.0];

#[derive(Clone, Copy, Debug)]
pub struct OfferChoice {
    pub kind: AttackKind,
    /// What the player would end up carrying - a fresh 1, or one more of what
    /// they already have.
    pub level: u8,
    pub body: Body,
}

impl OfferChoice {
    /// True when this is another level of what the player is already holding.
    pub fn is_upgrade(&self, current: AttackKind) -> bool {
        self.kind == current
    }
}

pub struct Offer {
    pub choices: [OfferChoice; OFFER_CHOICES],
    /// First tick on which a hit counts. A deadline rather than a countdown,
    /// for the same reason the dash uses one: the offer is created partway
    /// through a tick that has already run some of its work.
    pub live_from: i64,
}

impl Offer {
    /// True once hitting an option means taking it.
    pub fn armed(&self, timer: i64) -> bool {
        timer >= self.live_from
    }

    /// Draws three distinct options for a player carrying `current` at `level`.
    ///
    /// The pool is every kind they are not holding, plus one more level of the
    /// one they are - so a run can deepen a weapon instead of only swapping it,
    /// and a maxed-out weapon quietly stops being offered.
    #[allow(clippy::too_many_arguments)]
    pub fn roll(
        v: &Viewport,
        camera_x: f32,
        ground_y: f32,
        current: AttackKind,
        level: u8,
        timer: i64,
        rng: &mut Rng,
    ) -> Self {
        // At most the whole roster plus the level-up, so a fixed array does.
        let mut pool = [(AttackKind::Basic, 1u8); AttackKind::ALL.len() + 1];
        let mut n = 0;
        if current != AttackKind::Basic && level < MAX_LEVEL {
            pool[n] = (current, level + 1);
            n += 1;
        }
        for kind in AttackKind::ALL {
            if kind != current {
                pool[n] = (kind, 1);
                n += 1;
            }
        }

        // Partial shuffle: three draws is all that is needed, and it keeps the
        // generator's use proportional to what is taken rather than to the pool.
        for i in 0..OFFER_CHOICES.min(n) {
            let j = i + (rng.range(0, (n - i - 1) as i32).max(0) as usize);
            pool.swap(i, j);
        }

        let w = v.wper(5.0);
        let h = v.hper(10.0);
        let mut choices = [OfferChoice {
            kind: AttackKind::Basic,
            level: 1,
            body: Body::default(),
        }; OFFER_CHOICES];
        for (i, choice) in choices.iter_mut().enumerate() {
            let (kind, level) = pool[i.min(n.saturating_sub(1))];
            *choice = OfferChoice {
                kind,
                level,
                body: Body::new(
                    camera_x + v.wper(OFFER_X_PCT[i]) - w / 2.0,
                    ground_y - h,
                    w,
                    h,
                ),
            };
        }
        Self { choices, live_from: timer + OFFER_ARM_TICKS }
    }
}
