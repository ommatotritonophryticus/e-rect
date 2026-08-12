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

pub const MAX_PLAYERS: usize = 2;
/// HP a downed player comes back with at the start of the next wave.
pub const REVIVE_HP: f32 = 128.0;

/// How long a melee swing stays active. The JS used a 100 ms `setTimeout`.
pub const ATTACK_TICKS: i32 = 6;
/// Frames within which a second/third hit continues the combo.
pub const COMBO_WINDOW_TICKS: i64 = 60;

/// Popup lifetime in ticks.
pub const POPUP_LIFETIME: i64 = 30;

/// Concurrent enemies allowed on screen at once.
pub const MAX_CONCURRENT_ENEMIES: usize = 10;
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
