//! Tuning constants, ported 1:1 from the original `main.js`.
//!
//! Every value that used to live at the top of the JS file lives here so the two
//! versions can be diffed by eye when balance is changed.

/// Simulation runs at a fixed rate. The original was frame-tied (every velocity
/// was a per-frame delta), which made the game run at double speed on a 120 Hz
/// display. Fixing the tick rate keeps all the constants below meaning exactly
/// what they meant before while decoupling speed from the monitor.
pub const TICKS_PER_SEC: f64 = 60.0;
pub const TICK_SECONDS: f64 = 1.0 / TICKS_PER_SEC;

/// % of screen height where the floor line sits.
pub const GROUND_Y_PCT: f32 = 85.0;
/// % height of the top HUD band.
pub const CEILING_H_PCT: f32 = 10.0;
/// % height of the bottom HUD band.
pub const FLOOR_H_PCT: f32 = 15.0;

/// Divisor in the zombie chase-speed formula. Larger = zombies need a higher
/// score before they reach the player's own move speed. At 800 that crossover
/// lands around score ~11000 (roughly wave 10-15 in practice).
pub const ZOMBIE_CHASE_SPEED_RAMP: f32 = 800.0;

// --- enemy variant tuning ---
pub const ENRAGE_HP_THRESHOLD: f32 = 0.3;
pub const ENRAGE_SPEED_MULTIPLIER: f32 = 1.5;

/// How much faster a wounded player banks energy. At full health the multiplier
/// is 1; at nothing left it reaches 1 + this, so energy comes in three times as
/// fast. Linear in between, so there is no threshold to play around.
pub const ENERGY_DESPERATION_BONUS: f32 = 2.0;

/// The wave the flying boss owns. It arrives instead of the ground bosses that
/// wave would otherwise bring, and only ever one of it turns up.
pub const FLYING_BOSS_WAVE: i64 = 10;
/// Twice the *area* of an ordinary flyer, so each side grows by the square root
/// of two rather than doubling - doubling both sides would be four times the
/// area, not two.
pub const FLYING_BOSS_SIZE_SCALE: f32 = core::f32::consts::SQRT_2;

/* ---------------- flyer arc ---------------- */

/// The dip of a flyer's sine has to bring it down to the middle of a standing
/// player, so a grounded player can actually be reached. Working back from the
/// geometry: the player stands on the ground line and is 10% tall, so its
/// middle is at 80%; a flyer is half a player tall, so its top has to reach 75%.
/// Keeping the high point where it was (11%) fixes the centre and the swing.
pub const FLYER_ARC_TOP_PCT: f32 = 11.0;
pub const FLYER_ARC_BOTTOM_PCT: f32 = 75.0;
pub const FLYER_ARC_CENTER_PCT: f32 = (FLYER_ARC_TOP_PCT + FLYER_ARC_BOTTOM_PCT) / 2.0;
pub const FLYER_ARC_SWING_PCT: f32 = (FLYER_ARC_BOTTOM_PCT - FLYER_ARC_TOP_PCT) / 2.0;

/// How fast a player walks, as a percentage of the view per tick.
///
/// Named because two other things are defined against it: the drift behind the
/// menus, and how hard a hit throws a ground enemy sideways.
pub const PLAYER_MOVE_PCT: f32 = 1.0;

/// How fast the view slides behind the menus, as a fraction of a walk.
///
/// The title screen has no player to follow, so without this the parallax
/// skyline is a still image and the menu looks like a screenshot. Slow enough
/// that it reads as drift rather than as something happening.
pub const MENU_CAMERA_DRIFT: f32 = 0.3;

/* ---------------- camera and the open field ---------------- */

/// The field runs forever left and right. Only the camera's horizontal position
/// moves; vertically the view is fixed, which is why the ground, the ceiling and
/// the whole HUD keep working untouched.
///
/// How quickly the camera closes on where it wants to be, per tick. Following
/// the player exactly would jitter on every knockback.
pub const CAMERA_FOLLOW: f32 = 0.15;

/// Co-op players cannot walk further apart than this, as a percentage of the
/// view. One screen has to hold both of them, so the field is open to the pair,
/// not to each player separately.
pub const PLAYER_LEASH_PCT: f32 = 80.0;

/// Ground enemies further than this many screen widths from the nearest player
/// are moved back to the spawn ring. Without it one slow straggler wandering off
/// would hold a wave open indefinitely.
pub const ENEMY_RECYCLE_SCREENS: f32 = 2.5;

/* ---------------- parallax backdrop ---------------- */

/// `(parallax, shade)` per layer, farthest first.
///
/// Brightness follows aerial perspective: the distant layer sits closest to the
/// background colour and the near one is the darkest, so depth reads without
/// any other cue. Reversing that would make the far layer the loudest thing on
/// screen.
pub const BACKDROP_LAYERS: [(f32, f32); 3] = [
    (0.25, 0.75),
    (0.75, 0.50),
    // Only a tenth darker than the layer behind it: at 0.25 the nearest blocks
    // sat at the same value as the black ground band and the actor outlines,
    // and competed with the things that matter.
    (0.95, 0.45),
];
/// One block per slot; the slot is what makes the layout deterministic.
pub const BACKDROP_SLOT_PCT: f32 = 30.0;
pub const BACKDROP_MIN_W_PCT: f32 = 10.0;
pub const BACKDROP_MAX_W_PCT: f32 = 25.0;
pub const BACKDROP_MIN_H_PCT: f32 = 20.0;
pub const BACKDROP_MAX_H_PCT: f32 = 80.0;

/// Knockback when a player is hit: up, and away from whatever hit them.
/// Percent of the viewport per tick, so it scales with the screen like
/// everything else.
pub const KNOCKBACK_UP_PCT: f32 = 2.0;
pub const KNOCKBACK_AWAY_PCT: f32 = 2.0;
/// Per-tick decay of the sideways push. Without it a single hit would carry the
/// player most of the way across the screen, since the arc lasts ~40 ticks.
pub const KNOCKBACK_DECAY: f32 = 0.9;
pub const RUNT_SIZE_SCALE: f32 = 0.6;
pub const RUNT_HP: f32 = 128.0;
pub const RUNT_SPEED_MULTIPLIER: f32 = 1.6;
pub const JUMPER_JUMP_EVERY: i32 = 90;
pub const JUMPER_JUMP_POWER_PCT: f32 = 3.0;
pub const ARMORED_ARMOR: f32 = 0.25;
pub const SPLITTER_CHILD_COUNT: u32 = 2;
pub const TELEPORTER_TELEPORT_EVERY: i32 = 120;
/// The leaper: stands still, then throws itself at where the player was.
///
/// 701 ms is 42.06 ticks at 60 Hz, so 42 it is - 700 ms, the closest the fixed
/// clock can express.
pub const LEAPER_CROUCH_TICKS: i32 = 42;
pub const LEAPER_JUMP_POWER_PCT: f32 = 4.0;
/// A leap is aimed at the player, but not across the whole field: past this the
/// arc would read as teleporting rather than jumping.
pub const LEAPER_MAX_REACH_PCT: f32 = 60.0;

/// Shooters telegraph. The white line tracks the player; when it turns red the
/// aim is locked, and that last stretch is the window to move out of the way.
pub const SHOOTER_AIM_TICKS: i32 = 36;
/// 100 ms at 60 Hz.
pub const SHOOTER_LOCK_TICKS: i32 = 6;

pub const SHOOTER_FIRE_EVERY: i32 = 100;
pub const SHOOTER_PROJECTILE_DAMAGE: f32 = 20.0;

/* ---------------- melee reach ---------------- */

/// The wave at which the attack box stops growing.
///
/// It used to grow with score, capped at twice the body width - a cap score
/// reaches around wave 3, so a progression meant to be felt all run was over
/// before the roster had finished unlocking. Tying the ramp to the wave number
/// makes it legible and puts its end somewhere worth arriving at.
///
/// Set past the wave the roster finishes unlocking on, and past the wave the
/// crowd starts growing on, so reach is still arriving while the fight is
/// already getting harder rather than finishing before it starts.
pub const GUN_RAMP_WAVES: i64 = 15;

/// What the ramp adds to the box at full stretch, on top of the body width.
///
/// Sized against the ceiling below: a body is 5% of the view, so a full combo
/// at wave 10 spans `(5 + 25) * 2` = exactly 60%. The ramp ends where the clamp
/// begins rather than pushing against it for the rest of the run.
pub const GUN_EXTRA_MAX_PCT: f32 = 25.0;

/// Hard ceiling on the melee box, as a percentage of the view.
///
/// A grounded wave doubles reach and a full combo doubles it again; without
/// this the two together would put the box across most of the field and make
/// aiming meaningless.
pub const GUN_MAX_REACH_PCT: f32 = 60.0;

/// What one wall takes off a boss, as a fraction of its full health.
///
/// A flat 255 was an instant kill on anything ordinary and two percent of a
/// late boss, so the strongest tool in the game did nothing exactly where it
/// was wanted most. A fraction stays meaningful at every wave.
pub const FIELD_BOSS_FRACTION: f32 = 1.0 / 7.0;

/* ---------------- dash ---------------- */

/// How far one dash carries the player, as a percentage of the view.
pub const DASH_DISTANCE_PCT: f32 = 30.0;

/// Ticks a dash takes to travel that distance.
///
/// Ten is a floor, not a taste: the player and an ordinary enemy are both 5% of
/// the view wide, so at fewer than ten steps the player would move more than a
/// body per tick and pass clean through enemies without ever overlapping one.
/// Anything shorter needs a swept test instead of a per-tick one.
pub const DASH_TICKS: i32 = 10;

/// Ticks before another dash may start, counted from the end of the last.
pub const DASH_COOLDOWN_TICKS: i32 = 25;

/// A dash costs a bar divided by this - a third of the way to the next charge.
///
/// The bar refills many times over in a late wave, so on its own this would
/// stop limiting anything past wave 10 or so. The cooldown is what actually
/// paces the dash; this is what makes it trade against the wall.
pub const DASH_COST_DIVISOR: i64 = 3;

/// The upward shove a dash gives whatever it runs through. Same impulse as a
/// jump, so a thrown enemy is out of the way for about forty ticks.
pub const DASH_THROW_PCT: f32 = 2.0;

/// Added to a flyer's phase when a dash goes through it, in ticks of arc.
///
/// A flyer's height is not stored - it is `sin((timer - spawn_offset) / 50)`
/// recomputed every tick - so there is no y to push. Shifting the phase by half
/// a period (50π ≈ 157) moves it to the mirrored point of its own arc instead:
/// whatever side of the centre line it was on, it is now on the other.
pub const FLYER_DASH_PHASE_SHIFT: i64 = 157;

/* ---------------- the shedder ---------------- */

/// The wave the shedder joins the roster on.
///
/// It is the first variant that arrives after the roster was otherwise
/// complete, and it lands on the same wave the crowd starts growing on.
pub const SHEDDER_MIN_WAVE: i64 = 11;

/// How far from the player a hurt shedder lands, as a percentage of the view.
///
/// Past the melee ceiling on purpose: the whole point of the thing is that a
/// hit cannot be followed up, however far the player's reach has ramped. Sized
/// off that ceiling rather than written out, so raising one raises the other.
pub const SHEDDER_TELEPORT_PCT: f32 = GUN_MAX_REACH_PCT + 5.0;

/// Husks an ordinary shedder keeps standing at once.
///
/// Replacing rather than accumulating is what stops one enemy from slowly
/// paving the field with hazards it never has to defend.
pub const SHEDDER_HUSKS: usize = 1;

/// Contact damage from a husk - the same as the enemy that left it. It is a
/// piece of that enemy, not a lesser one.
pub const HUSK_DAMAGE: f32 = 16.0;

/// How dark a husk is drawn against its parent's colour, so the two read as
/// the same thing without needing a second entry in the palette.
pub const HUSK_SHADE: f32 = 0.55;

/// The wave the shedder boss may own, in place of that wave's ground bosses.
pub const SHEDDER_BOSS_WAVE: i64 = 15;

/// The wave whose boss health the shedder boss borrows.
///
/// It fights by covering ground rather than by soaking damage, so it keeps the
/// first boss's health instead of its own wave's - which would be three times
/// as much, spread over an enemy that is never standing still to receive it.
pub const SHEDDER_BOSS_HEALTH_WAVE: i64 = 5;

/// Husks the shedder boss keeps standing: every one it has ever left.
///
/// Its health bounds this on its own - a husk per hit, and it has a fixed
/// number of hits in it - so there is no runaway to guard against.
pub const SHEDDER_BOSS_HUSKS: usize = usize::MAX;

pub const MAX_PLAYERS: usize = 2;
/// HP a downed player comes back with at the start of the next wave.
pub const REVIVE_HP: f32 = 128.0;

/// How long a melee swing stays active. The JS used a 100 ms `setTimeout`.
pub const ATTACK_TICKS: i32 = 6;
/// Frames within which a second/third hit continues the combo.
pub const COMBO_WINDOW_TICKS: i64 = 60;

/// Popup lifetime in ticks.
pub const POPUP_LIFETIME: i64 = 30;

/* ---------------- how many at once ---------------- */

/// Concurrent enemies allowed on screen for the first ten waves.
pub const MAX_CONCURRENT_ENEMIES: usize = 10;

/// The wave the crowd starts growing on.
///
/// The first ten waves are where the roster unlocks - a variant a wave from
/// wave 2 to wave 9 - so the thing that changes there is *what* turns up. Once
/// the player has met everything, the lever left is how much of it is on screen
/// at once, and that is what this ramp is.
pub const CROWD_RAMP_FIRST_WAVE: i64 = 11;

/// Waves per extra enemy on screen.
pub const CROWD_RAMP_EVERY: i64 = 2;

/// Ceiling on the crowd, as a multiple of the opening figure.
pub const CROWD_MAX_MULTIPLE: usize = 2;

/// Concurrent enemies allowed on screen during `wave`.
///
/// Flat until [`CROWD_RAMP_FIRST_WAVE`], then one more every
/// [`CROWD_RAMP_EVERY`] waves until it has doubled - which lands on wave 29 and
/// stays there. Growing without a ceiling would eventually make the screen a
/// wall of enemies rather than a fight, and on a PSP it would also be the first
/// thing to cost frames.
pub const fn max_concurrent_enemies(wave: i64) -> usize {
    if wave < CROWD_RAMP_FIRST_WAVE {
        return MAX_CONCURRENT_ENEMIES;
    }
    // +1 on the first ramp wave itself, hence the trailing increment.
    let extra = ((wave - CROWD_RAMP_FIRST_WAVE) / CROWD_RAMP_EVERY + 1) as usize;
    let ceiling = MAX_CONCURRENT_ENEMIES * CROWD_MAX_MULTIPLE;
    if MAX_CONCURRENT_ENEMIES + extra > ceiling {
        ceiling
    } else {
        MAX_CONCURRENT_ENEMIES + extra
    }
}
/// Seconds counted down between waves.
pub const WAVE_COUNTDOWN_SECONDS: i32 = 10;
/// How long the result screen holds after a run ends, in ticks. Input is
/// ignored throughout, so a button held at the moment of death cannot skip it.
pub const GAME_OVER_TICKS: i32 = (1.2 * TICKS_PER_SEC) as i32;

pub const MENU_ROW_H_PCT: f32 = 9.0;
pub const GAMEPAD_DEADZONE: f32 = 0.5;

/// The eight selectable player colours, as (name, r, g, b).
pub const PLAYER_COLORS: [(&str, f32, f32, f32); 8] = [
    ("GREEN", 0.0, 255.0, 0.0),
    ("CYAN", 0.0, 220.0, 255.0),
    ("MAGENTA", 255.0, 0.0, 220.0),
    ("ORANGE", 255.0, 150.0, 0.0),
    ("YELLOW", 255.0, 240.0, 0.0),
    ("WHITE", 255.0, 255.0, 255.0),
    ("BLUE", 80.0, 130.0, 255.0),
    ("RED", 255.0, 60.0, 60.0),
];
