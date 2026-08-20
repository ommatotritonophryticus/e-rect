//! Players, enemies and short-lived effects.
//!
//! The JS attached per-variant behaviour as an `onTick` closure on each enemy.
//! Closures that mutate the whole world do not translate to Rust ownership, so
//! behaviour is data the simulation reads instead - same effect, and it keeps
//! every enemy a plain value that can live in a `Vec`.
//!
//! That data is a *set*, not a choice: an enemy may hop and shoot and shed all
//! at once. Every named variant below is a preset over the same fields, which
//! is why they cost nothing to keep alongside enemies built by rolling dice.

use alloc::string::String;
use alloc::vec::Vec;

use crate::attack::{AttackKind, HeightAnchor};
use crate::boon::Boons;
use crate::color::Rgb;
use crate::config::*;
use crate::geom::{Body, Viewport};

/* ------------------------------------------------------------------ *
 * Player
 * ------------------------------------------------------------------ */

/// What a hit is allowed to reach through.
///
/// One thing in the game is allowed past a dash, and it is worth a type rather
/// than a bare flag at the call site: a `true` there says nothing about which
/// way round the rule runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Stopped by every protection there is.
    Normal,
    /// Reaches a dashing player anyway - a raised wall still stops it.
    ///
    /// The shedder's husk, and only that. It takes no damage and it does not
    /// move, which makes it the one answer to using the dash as a crowd-clear;
    /// if a dash made it harmless too, there would be nothing on the field a
    /// dash could not simply drive through.
    ThroughDash,
}

pub struct Player {
    pub index: usize,
    pub body: Body,
    pub ax: f32,
    pub ay: f32,
    /// Untouchable until the next landing. Bought with a charge on a no-wall
    /// wave, where the slam button does this instead of putting up a wall.
    pub invulnerable: bool,
    /// Sideways velocity from being hit. Separate from `ax`, which is rebuilt
    /// from the pad every tick and so cannot hold anything. Cleared on landing,
    /// which is what stops the player skating along the floor.
    pub knockback_x: f32,
    pub hp: f32,
    pub hpmax: f32,
    pub color: Rgb,
    pub scheme: usize,

    pub kills: u32,
    pub score: i64,
    pub energy: i64,
    pub super_charges: i64,
    pub attacks_since_power_up: i64,

    /// Ticks of dash still to travel; zero when not dashing.
    pub dash_ticks: i32,
    /// Which way the dash in progress is going, +1 or -1.
    pub dash_dir: f32,
    /// Ticks until another dash may start. Also drives the indicator.
    pub dash_cooldown: i32,
    /// Last tick on which the dash still shoves and still cannot be touched.
    ///
    /// A deadline rather than a countdown: the dash ends partway through a tick
    /// that has already done some of its work, and a counter set there would be
    /// decremented by that same tick and come up one frame short.
    pub dash_grace_until: i64,

    /// Standing upgrades this run has picked up.
    pub boons: Boons,
    /// Jumps left before the next landing. Only ever above zero with
    /// [`Boon::DoubleJump`], and spent by jumping off nothing.
    pub air_jumps: u8,
    /// First tick the shield can absorb again. A deadline rather than a
    /// counter, so it survives the clock going back to zero at the start of a
    /// run - a "last used at" would read as freshly spent there.
    pub shield_ready_at: i64,

    pub grounded: bool,
    pub facing_right: bool,
    pub attack_ticks: i32,
    pub combo: u32,
    pub combo_timestamp: i64,
    pub dead: bool,

    pub gun: Body,
    /// The second box of a two-sided swing, behind the player. `None` for every
    /// kind that only reaches one way.
    pub gun_back: Option<Body>,
    /// Which attack the upgrades have left this player with.
    pub attack: AttackKind,
    /// How far that upgrade has been taken, from 1 to the kind's maximum.
    pub attack_level: u8,
    /// Counts up once per swing. Enemies remember the last one that hit them,
    /// which is how a kind that may only land once per swing knows.
    pub strike_id: u32,
    pub field: UltimateField,
}

impl Player {
    pub fn new(index: usize, v: &Viewport, color: Rgb, scheme: usize) -> Self {
        let mut player = Self {
            index,
            body: Body::new(v.wper(50.0), v.hper(10.0), v.wper(5.0), v.hper(10.0)),
            ax: 0.0,
            ay: 0.0,
            invulnerable: false,
            knockback_x: 0.0,
            hp: 255.0,
            hpmax: 255.0,
            color,
            scheme,
            kills: 0,
            score: 0,
            energy: 0,
            super_charges: 1,
            attacks_since_power_up: 0,
            dash_ticks: 0,
            dash_dir: 1.0,
            dash_cooldown: 0,
            dash_grace_until: -1,
            boons: Boons::default(),
            air_jumps: 0,
            shield_ready_at: 0,
            grounded: false,
            facing_right: true,
            attack_ticks: 0,
            combo: 0,
            combo_timestamp: 0,
            dead: false,
            gun: Body::default(),
            gun_back: None,
            attack: AttackKind::Basic,
            attack_level: 1,
            strike_id: 0,
            field: UltimateField::default(),
        };
        player.reset(v, v.wper(50.0));
        player
    }

    pub fn reset(&mut self, v: &Viewport, spawn_x: f32) {
        self.body = Body::new(spawn_x, v.hper(10.0), v.wper(5.0), v.hper(10.0));
        self.ax = 0.0;
        self.ay = 0.0;
        self.invulnerable = false;
        self.knockback_x = 0.0;
        self.hp = 255.0;
        self.kills = 0;
        self.dash_ticks = 0;
        self.dash_dir = 1.0;
        self.dash_cooldown = 0;
        self.dash_grace_until = -1;
        self.grounded = false;
        self.facing_right = true;
        self.attack_ticks = 0;
        self.energy = 0;
        self.score = 0;
        self.super_charges = 1;
        self.attacks_since_power_up = 0;
        self.combo = 0;
        self.combo_timestamp = 0;
        self.dead = false;
        self.gun_back = None;
        self.attack = AttackKind::Basic;
        self.attack_level = 1;
        // Boons belong to a run, exactly as the weapon does. `revive` is the
        // one that must not touch them: dying between waves costs health and
        // position, never what the run has earned.
        self.boons = Boons::default();
        self.air_jumps = 0;
        self.shield_ready_at = 0;
        self.field = UltimateField::default();
    }

    /// Comes back for the next wave keeping score and charges.
    pub fn revive(&mut self, v: &Viewport, spawn_x: f32) {
        self.dead = false;
        self.hp = REVIVE_HP;
        self.body.x = spawn_x;
        self.body.y = v.hper(10.0);
        self.ax = 0.0;
        self.ay = 0.0;
        self.invulnerable = false;
        self.knockback_x = 0.0;
        self.attack_ticks = 0;
        self.dash_ticks = 0;
        self.dash_cooldown = 0;
        self.dash_grace_until = -1;
        self.field = UltimateField::default();
    }

    pub fn attacking(&self) -> bool {
        self.attack_ticks > 0
    }

    pub fn dashing(&self) -> bool {
        self.dash_ticks > 0
    }

    /// Every state that means "this cannot hurt me", in one place.
    ///
    /// Three of these used to be checked only where an enemy *body* touched the
    /// player. That left the two sources which do not go through those sites -
    /// a shot and a shedder's hazard - reaching straight through a raised wall
    /// and through a dash.
    pub fn untouchable(&self, timer: i64, reach: Reach) -> bool {
        if self.invulnerable || self.field.readiness || self.field.active {
            return true;
        }
        reach == Reach::Normal && self.in_dash_window(timer)
    }

    /// True while a shield is held and ready to take the next touch.
    pub fn shield_up(&self, timer: i64) -> bool {
        self.boons.shield && timer >= self.shield_ready_at
    }

    /// While this holds, the dash shoves what it touches instead of trading
    /// damage with it. Outlasts the travel by [`DASH_GRACE_TICKS`].
    pub fn in_dash_window(&self, timer: i64) -> bool {
        self.dash_ticks > 0 || timer <= self.dash_grace_until
    }

    /// Colour of the melee box.
    ///
    /// At rest that box sits inside the body, offset to whichever way the
    /// player faces - it is already the direction indicator, so it is where the
    /// dash cooldown belongs too rather than on a second square competing with
    /// it. Red the instant a dash ends, easing back to white as the cooldown
    /// runs out.
    ///
    /// A swing is always white. Mid-attack the box is what the player reads for
    /// reach, and recolouring it then would be saying two things with one
    /// shape at the moment it matters most.
    ///
    /// Worked out here rather than in each renderer, so the two frontends
    /// cannot drift apart on what it means.
    pub fn gun_color(&self) -> Rgb {
        if self.attacking() {
            return Rgb::new(255.0, 255.0, 255.0);
        }
        let left = if DASH_COOLDOWN_TICKS > 0 {
            (self.dash_cooldown as f32 / DASH_COOLDOWN_TICKS as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Red stays full; the other two channels come back up as it recovers.
        let rest = 255.0 * (1.0 - left);
        Rgb::new(255.0, rest, rest)
    }

    /// Never 0: a zero threshold would blank the HUD bar and let charges
    /// accumulate every tick.
    pub fn energy_needed(&self, wave: i64) -> i64 {
        let raw = (25 + wave * 10) as f32
            * ((self.super_charges + self.attacks_since_power_up) as f32 / 2.0);
        (raw as i64).max(1)
    }

    /// Melee box position/size; widens with the wave and doubles on a full
    /// combo. `reach` scales the attack box; a grounded wave doubles it to make
    /// up for the mobility the player lost.
    pub fn update_gun(&mut self, v: &Viewport, reach: f32, wave: i64) {
        let b = self.body;
        let top = b.y + v.hper(0.5);
        let base_h = b.h / 2.0;

        // The box at rest is the same for every kind: it is the direction
        // indicator and the dash readout, not a preview of the swing.
        //
        // A kind that throws or places its attack never leaves that state - it
        // has nothing held out to draw, and the thing it made is in the world.
        if !self.attacking() || self.attack.detached() {
            self.gun.h = base_h;
            self.gun.y = top;
            self.gun.w = b.w / 2.0 * reach;
            self.gun.x = if self.facing_right {
                b.x + b.w / 2.0 - v.wper(0.5)
            } else {
                b.x + v.wper(0.5)
            };
            self.gun_back = None;
            return;
        }

        let kind = self.attack;
        let level = self.attack_level;

        // Height first: a kind that trades height for something else anchors
        // the edge it is not giving up.
        self.gun.h = base_h * kind.height_scale(level);
        self.gun.y = match kind.height_anchor() {
            HeightAnchor::Top => top,
            HeightAnchor::Bottom => top + base_h - self.gun.h,
        };

        // How far through the swing we are, 0 on the first tick and 1 on the
        // last. Only the lunge reads it, but it costs nothing to always have.
        let total = kind.swing_ticks(ATTACK_TICKS).max(1) as f32;
        let progress = ((total - self.attack_ticks as f32) / total).clamp(0.0, 1.0);

        let ramp = wave.clamp(1, GUN_RAMP_WAVES) as f32 / GUN_RAMP_WAVES as f32;
        let extra = v.wper(GUN_EXTRA_MAX_PCT) * ramp;
        let combo = if self.combo == 2 { 2.0 } else { 1.0 };
        self.gun.w = (b.w + extra) * combo * reach * kind.length_scale(level, progress);
        // Reach and the combo each double the box; both at once on a late wave
        // would otherwise span the field.
        self.gun.w = self.gun.w.min(v.wper(GUN_MAX_REACH_PCT));
        self.gun.x = if self.facing_right {
            b.x + b.w
        } else {
            b.x - self.gun.w
        };

        // The mirrored box of a two-sided swing, sharing every dimension.
        self.gun_back = kind.two_sided().then(|| {
            let mut back = self.gun;
            back.x = if self.facing_right {
                b.x - self.gun.w
            } else {
                b.x + b.w
            };
            back
        });
    }

    /// Every box this player's swing is currently putting into the world.
    pub fn strike_boxes(&self) -> impl Iterator<Item = &Body> {
        core::iter::once(&self.gun).chain(self.gun_back.iter())
    }

    /// Where a held swing of this length would land.
    ///
    /// The kinds that place something instead of holding it still aim with the
    /// same geometry, so the button keeps meaning "put damage over there" - but
    /// their `update_gun` never leaves the resting box, so there is nothing to
    /// read off `gun`.
    pub fn projected_swing(&self, v: &Viewport, reach: f32, wave: i64, length: f32) -> Body {
        let b = self.body;
        let ramp = wave.clamp(1, GUN_RAMP_WAVES) as f32 / GUN_RAMP_WAVES as f32;
        let extra = v.wper(GUN_EXTRA_MAX_PCT) * ramp;
        let w = ((b.w + extra) * reach * length).min(v.wper(GUN_MAX_REACH_PCT));
        let x = if self.facing_right { b.x + b.w } else { b.x - w };
        Body::new(x, b.y + v.hper(0.5), w, b.h / 2.0)
    }
}

/* ------------------------------------------------------------------ *
 * Thrown and placed attacks
 * ------------------------------------------------------------------ */

/// A square the player threw instead of swinging.
///
/// Kept apart from the shooter's `Projectile` on purpose: that one damages the
/// player and this one damages enemies, and folding them together would put an
/// owner check on every collision in the game to save one struct.
#[derive(Clone, Copy, Debug)]
pub struct Bullet {
    pub body: Body,
    /// Per-tick travel. Zeroed the moment it stops on something.
    pub ax: f32,
    pub owner: usize,
    /// Fixed when it was fired, so a later upgrade cannot change a shot that is
    /// already in the air.
    pub damage: f32,
    /// Multiplies the sideways throw. Zero for a bullet: one that pushed its
    /// target sideways would shove it straight out of itself.
    pub knockback: f32,
    /// Multiplies the upward throw, which a bullet does want.
    pub lift: f32,
    /// Counts down only after it has stopped; -1 while still travelling.
    pub ticks_left: i32,
    pub dead: bool,
}

impl Bullet {
    /// Square in pixels - both `Body` dimensions are pixels, so the shorter
    /// side of the swing box it replaces gives a real square on any screen.
    pub fn new(
        owner: usize,
        from: &Body,
        facing_right: bool,
        v: &Viewport,
        damage: f32,
        knockback: f32,
        lift: f32,
    ) -> Self {
        let side = from.h / 2.0;
        let x = if facing_right { from.x + from.w } else { from.x - side };
        Self {
            body: Body::new(x, from.y + v.hper(0.5), side, side),
            ax: if facing_right { 1.0 } else { -1.0 } * v.wper(PLAYER_MOVE_PCT * BULLET_SPEED_MULT),
            owner,
            damage,
            knockback,
            lift,
            ticks_left: -1,
            dead: false,
        }
    }

    pub fn flying(&self) -> bool {
        self.ticks_left < 0
    }

    /// Stops it where it is and starts the short life it has left.
    pub fn stop(&mut self) {
        self.ax = 0.0;
        self.ticks_left = BULLET_HIT_TICKS;
    }
}

/// A box the player left standing.
#[derive(Clone, Copy, Debug)]
pub struct Trap {
    pub body: Body,
    pub owner: usize,
    pub damage: f32,
    /// Multiplies the sideways and upward throw, carried from the moment it was
    /// placed for the same reason its damage is.
    pub knockback: f32,
    pub lift: f32,
    pub ticks_left: i32,
}

/* ------------------------------------------------------------------ *
 * Ultimate field
 * ------------------------------------------------------------------ */

#[derive(Clone, Copy, Debug, Default)]
pub struct UltimateField {
    pub body: Body,
    pub active: bool,
    pub readiness: bool,
}

impl UltimateField {
    pub fn activate(&mut self, player: &Body, v: &Viewport) {
        self.active = true;
        self.body = Body::new(
            player.x - player.w / 2.0,
            v.hper(GROUND_Y_PCT) - player.h / 2.0,
            player.w * 2.0,
            player.h / 2.0,
        );
    }

    pub fn grow(&mut self, v: &Viewport, player: &Body) {
        if self.body.w < player.w * 4.0 {
            self.body.w += v.wper(0.6);
            self.body.x -= v.wper(0.3);
            self.body.y = 0.0;
            self.body.h = v.hper(GROUND_Y_PCT);
        } else {
            self.active = false;
        }
    }
}

/* ------------------------------------------------------------------ *
 * Enemies
 * ------------------------------------------------------------------ */

/// The shedder's signature colour.
///
/// Green is the one hue no other variant claims. The husks it leaves are drawn
/// in the same colour darkened, so the pair reads as one thing without costing
/// a second entry in an already crowded palette.
pub const SHEDDER_COLOR: Rgb = Rgb::new(90.0, 200.0, 90.0);

/// A leap in progress: the wind-up, then the committed arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leap {
    /// Ticks left standing dead still. That stillness is the tell.
    pub crouch: i32,
    /// While true the trajectory is locked and nothing steers it.
    pub airborne: bool,
}

/// How an enemy gets about. Exactly one of these, always.
///
/// Movement is a choice where behaviour is a set, and the difference is not
/// arbitrary: two ways of moving would both write the same velocity on the same
/// tick and one would silently win. Making it an enum says that in the type
/// instead of guarding for it at runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Movement {
    /// Walks at whoever is nearest. What every enemy did before there was a
    /// choice.
    #[default]
    Run,
    /// Walks, and hops on a timer.
    Hop { cooldown: i32 },
    /// Stands still, then throws itself at where the player was.
    Leap(Leap),
    /// Rides the same sine an ordinary flyer does, gravity ignored. `offset`
    /// fixes where in that wave it starts.
    Fly { offset: i64 },
}

/// What an enemy does beyond walking at whoever is nearest.
///
/// A set rather than a choice. It used to be an enum, which made every enemy
/// exactly one thing - and the roster paid for it: ten variants, of which seven
/// differ only in which single behaviour they carry, because size, speed and
/// armour could be combined and behaviour could not.
///
/// Each entry carries its own state, so two of them running at once do not
/// share a counter and cannot tread on each other. How the enemy *moves* is a
/// separate axis - see [`Movement`] - because those genuinely are exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Behaviors {
    /// Ticks until the next shot.
    pub shoot: Option<i32>,
    /// Leaves a blast and jumps to the edge on contact, and once on damage.
    pub blink: bool,
    /// Blinks out of reach on every hit it survives, leaving a hazard behind.
    pub shed: bool,
}

impl Behaviors {
    /// Nothing but the walk every enemy has.
    pub fn plain() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug)]
pub struct Zombie {
    pub body: Body,
    pub ax: f32,
    pub ay: f32,
    pub hp: f32,
    pub hpmax: f32,
    pub color: Rgb,

    pub is_boss: bool,
    /// Built by rolling rather than picked off the roster. Drives nothing in
    /// the simulation - it is what tells a renderer to mark it, since a rolled
    /// enemy has no signature colour to be recognised by.
    pub elite: bool,
    /// Answers every blow it survives with young. Never set on the young.
    pub broods: bool,
    /// What killing it pays. Carried rather than derived, so how tough a thing
    /// is and what it is worth can be set apart from each other.
    pub reward: i64,
    pub armor: f32,
    pub splits_into: u32,
    pub speed_multiplier: f32,
    pub enrages: bool,
    pub movement: Movement,
    pub behaviors: Behaviors,
    /// Set by the first hit this one takes; a blinker uses it to leave exactly
    /// once for damage, however many hits follow.
    pub hurt_once: bool,
    /// The last swing that landed on this one. A kind that may only hit each
    /// enemy once per swing compares against it; 0 means nothing has.
    pub last_strike: u32,
    /// Where a shooter's sight points, in world coordinates. Kept across ticks
    /// so the last stretch before the shot can be locked and dodged.
    pub aim: Option<(f32, f32)>,

    /// Standing hazards this enemy has left behind, in world coordinates.
    ///
    /// Owned by the enemy rather than kept in a list of their own, which gets
    /// three of the rules for free: they vanish when it dies, they never count
    /// towards the live-enemy total that decides when a wave is over, and no
    /// identity has to be tracked to tell whose is whose.
    pub husks: Vec<Body>,
    /// How many husks may stand at once. One for an ordinary shedder, so a new
    /// one replaces the last; effectively unbounded for the boss.
    pub max_husks: usize,
}

/// One dot of a shooter's sight. A dotted line because both renderers can only
/// lay down axis-aligned rectangles; a solid diagonal is not expressible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimDot {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    /// The aim is locked and the shot is imminent - drawn red.
    pub hot: bool,
}

impl Zombie {
    fn base(x: f32, y: f32, v: &Viewport, color: Rgb) -> Self {
        Self {
            body: Body::new(x, y, v.wper(5.0), v.hper(10.0)),
            ax: 0.0,
            ay: 0.0,
            hp: 255.0,
            hpmax: 255.0,
            color,
            is_boss: false,
            elite: false,
            broods: false,
            reward: 6,
            armor: 1.0,
            splits_into: 0,
            speed_multiplier: 1.0,
            enrages: false,
            movement: Movement::default(),
            behaviors: Behaviors::default(),
            hurt_once: false,
            last_strike: 0,
            aim: None,
            husks: Vec::new(),
            max_husks: 0,
        }
    }

    /// Placed just past the left or right edge of the view. The x is relative
    /// to the view; the caller shifts it into the world by adding the camera.
    ///
    /// The colour comes from the caller rather than a roll here. Every variant
    /// has a signature colour and colour is the only thing telling them apart,
    /// so a base enemy drawing at random out of the same range could turn up
    /// wearing a jumper's orange and lie to the player about what it does.
    pub fn from_edge(v: &Viewport, rng: &mut Rng, color: Rgb) -> Self {
        let x = if rng.flip() {
            -v.wper(5.0)
        } else {
            v.wper(105.0)
        };
        Self::base(x, v.hper(75.0), v, color)
    }

    pub fn boss(v: &Viewport, wave: i64, rng: &mut Rng) -> Self {
        let x = if rng.flip() {
            -v.wper(5.0)
        } else {
            v.wper(110.0)
        };
        let mut boss = Self::base(x, v.hper(60.0), v, Rgb::new(255.0, 255.0, 255.0));
        boss.is_boss = true;
        boss.hp = 255.0 * (10.0 * (wave as f32 / 5.0));
        boss.hpmax = boss.hp;
        boss.body.h += v.hper(5.0);
        boss.body.w += v.wper(5.0);
        boss
    }

    /// Half-size offspring of a killed splitter. `splits_into` is cleared so
    /// there is no chain reaction.
    pub fn child(parent: &Zombie, v: &Viewport, rng: &mut Rng) -> Self {
        let mut child = Self::base(parent.body.x, parent.body.y, v, parent.color);
        child.body.w = parent.body.w / 2.0;
        child.body.h = parent.body.h / 2.0;
        child.hp = libm::floorf(parent.hpmax / 2.0).max(1.0);
        child.hpmax = child.hp;
        child.ax = if rng.flip() {
            v.wper(0.5)
        } else {
            -v.wper(0.5)
        };
        child
    }

    pub fn runt(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(255.0, 220.0, 80.0));
        z.body.w *= RUNT_SIZE_SCALE;
        z.body.h *= RUNT_SIZE_SCALE;
        z.hp = RUNT_HP;
        z.hpmax = RUNT_HP;
        z.speed_multiplier = RUNT_SPEED_MULTIPLIER;
        z
    }

    /// Winds up on the spot, then throws itself at the player.
    pub fn leaper(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(255.0, 90.0, 200.0));
        z.movement = Movement::Leap(Leap {
            crouch: LEAPER_CROUCH_TICKS,
            airborne: false,
        });
        z
    }

    pub fn jumper(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(255.0, 140.0, 0.0));
        // staggered so a group of jumpers does not move in lockstep
        z.movement = Movement::Hop {
            cooldown: rng.range(0, JUMPER_JUMP_EVERY),
        };
        z
    }

    pub fn armored(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(150.0, 150.0, 160.0));
        z.armor = ARMORED_ARMOR;
        z
    }

    pub fn frenzied(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(180.0, 20.0, 20.0));
        z.enrages = true;
        z
    }

    pub fn splitter(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(170.0, 60.0, 220.0));
        z.splits_into = SPLITTER_CHILD_COUNT;
        z
    }

    /// Refuses to be fought where it stands: touch it, or land the first hit on
    /// it, and it leaves a blast behind and reappears at the edge of the field.
    pub fn blinker(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(220.0, 40.0, 40.0));
        z.behaviors.blink = true;
        z
    }

    /// Refuses to be traded with: every hit sends it out of reach and leaves a
    /// husk standing where the hit landed.
    pub fn shedder(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, SHEDDER_COLOR);
        z.behaviors.shed = true;
        z.max_husks = SHEDDER_HUSKS;
        z
    }

    /// The wave-15 boss: a shedder that keeps every husk it leaves, with the
    /// first boss's health rather than its own wave's.
    pub fn shedder_boss(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::boss(v, SHEDDER_BOSS_HEALTH_WAVE, rng);
        z.behaviors.shed = true;
        z.max_husks = SHEDDER_BOSS_HUSKS;
        z
    }

    pub fn shooter(v: &Viewport, rng: &mut Rng) -> Self {
        let mut z = Self::from_edge(v, rng, Rgb::new(60.0, 200.0, 220.0));
        z.behaviors.shoot = Some(rng.range(0, SHOOTER_FIRE_EVERY));
        z
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlyerBehavior {
    None,
    /// Re-rolls its edge and wave phase instead of drifting, so it blinks around.
    Teleporter {
        cooldown: i32,
    },
}

#[derive(Clone, Debug)]
pub struct Flyer {
    /// Set the first time this flyer is damaged; a teleporter uses it to blink
    /// away exactly once, on that first hit.
    pub hurt_once: bool,
    /// The last swing that landed on this one; see [`Zombie::last_strike`].
    pub last_strike: u32,
    pub body: Body,
    pub ax: f32,
    pub ay: f32,
    pub hp: f32,
    pub hpmax: f32,
    pub color: Rgb,
    pub spawn_offset: i64,
    pub behavior: FlyerBehavior,
    /// The wave-10 flying boss. It blinks away on *every* hit, not just the
    /// first, and pays out at boss rates.
    pub is_boss: bool,
}

impl Flyer {
    pub fn from_edge(v: &Viewport, size_ref: &Body, timer: i64, rng: &mut Rng) -> Self {
        let color = Rgb::new(
            rng.range(100, 255) as f32,
            rng.range(100, 255) as f32,
            rng.range(100, 255) as f32,
        );
        let x = if rng.flip() { v.w - size_ref.w } else { 0.0 };
        Self {
            hurt_once: false,
            last_strike: 0,
            is_boss: false,
            body: Body::new(x, 0.0, size_ref.w, size_ref.h / 2.0),
            ax: v.wper(0.5),
            ay: 0.0,
            hp: 255.0,
            hpmax: 255.0,
            color,
            spawn_offset: -timer,
            behavior: FlyerBehavior::None,
        }
    }

    /// Wave 10's flying boss: twice the area of a normal flyer, the same health
    /// as one, and impossible to pin down - it teleports on every hit it takes.
    pub fn flying_boss(v: &Viewport, size_ref: &Body, timer: i64, rng: &mut Rng) -> Self {
        let mut f = Self::from_edge(v, size_ref, timer, rng);
        f.is_boss = true;
        f.color = Rgb::new(255.0, 255.0, 255.0);
        f.body.w *= FLYING_BOSS_SIZE_SCALE;
        f.body.h *= FLYING_BOSS_SIZE_SCALE;
        f
    }

    pub fn teleporter(v: &Viewport, size_ref: &Body, timer: i64, rng: &mut Rng) -> Self {
        let mut f = Self::from_edge(v, size_ref, timer, rng);
        f.color = Rgb::new(255.0, 255.0, 120.0);
        f.behavior = FlyerBehavior::Teleporter {
            cooldown: rng.range(0, TELEPORTER_TELEPORT_EVERY),
        };
        f
    }
}

/* ------------------------------------------------------------------ *
 * Effects
 * ------------------------------------------------------------------ */

#[derive(Clone, Debug)]
pub struct Explosion {
    pub body: Body,
    pub max_w: f32,
    pub finished: bool,
    /// What it does to anything it touches. Zero for the blast every death
    /// leaves, which is scenery; only a combo finisher makes a live one.
    pub damage: f32,
    /// Its identity, from the same pool swings draw from, so a growing blast
    /// hits each enemy once rather than on every tick it covers them.
    pub strike: u32,
}

impl Explosion {
    pub fn new(x: f32, y: f32, v: &Viewport) -> Self {
        Self {
            body: Body::new(x, y, 0.0, 0.0),
            max_w: v.wper(20.0),
            finished: false,
            damage: 0.0,
            strike: 0,
        }
    }

    /// A blast that hurts. Only a kill that finished a combo makes one.
    pub fn lethal(x: f32, y: f32, v: &Viewport, damage: f32, strike: u32) -> Self {
        Self { damage, strike, ..Self::new(x, y, v) }
    }

    pub fn update(&mut self, v: &Viewport) {
        if self.body.w < self.max_w {
            self.body.x -= v.wper(0.1);
            self.body.y -= v.hper(0.1);
            self.body.w += v.wper(0.2);
            self.body.h += v.hper(0.2);
        } else {
            self.finished = true;
        }
    }
}

#[derive(Clone, Debug)]
pub struct Projectile {
    pub body: Body,
    pub ax: f32,
    pub ay: f32,
    pub damage: f32,
    pub dead: bool,
}

impl Projectile {
    /// `view_left`/`view_right` are the world edges of what is on screen: the
    /// field is open, so a projectile is spent when it leaves the view rather
    /// than when it leaves the screen's own coordinates.
    pub fn update(&mut self, v: &Viewport, view_left: f32, view_right: f32) {
        self.body.x += self.ax;
        self.body.y += self.ay;
        if self.body.x + self.body.w < view_left - v.wper(20.0)
            || self.body.x > view_right + v.wper(20.0)
            || self.body.y < -self.body.h
            || self.body.y > v.h
        {
            self.dead = true;
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScorePopup {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub created_at: i64,
}

impl ScorePopup {
    pub fn age(&self, timer: i64) -> i64 {
        timer - self.created_at
    }

    pub fn is_expired(&self, timer: i64) -> bool {
        self.age(timer) > POPUP_LIFETIME
    }
}

/* ------------------------------------------------------------------ *
 * Rng - small xorshift so runs are reproducible and no extra crate is
 * needed just for spawn rolls.
 * ------------------------------------------------------------------ */

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Inclusive on both ends, like the original `rand(min, max)`.
    pub fn range(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        let span = (max - min + 1) as u64;
        min + (self.next_u64() % span) as i32
    }

    pub fn flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// A fresh 64-bit value, for seeding something else.
    pub fn next_seed(&mut self) -> u64 {
        self.next_u64()
    }

    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 11) as f32 / (1u64 << 53) as f32
    }
}
