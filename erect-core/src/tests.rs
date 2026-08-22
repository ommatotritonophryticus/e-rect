//! Simulation tests. These drive `Game` headlessly (no window, no rendering),
//! which is how the JS original was verified too.

use alloc::vec;
use alloc::vec::Vec;

use alloc::string::{String, ToString};
use crate::attack::{AttackKind, MAX_LEVEL};
use crate::recipe::{unlock, MoveKind, Recipe, Size, BROOD_MAX, ELITE_COLOR, ELITE_HP};
use crate::boon::{Boon, Boons, WallMod, SHIELD_COOLDOWN_TICKS};
use crate::config::{
    boss_hp, elite_group_size, elites_in_wave as elite_count, wave_budget, ROLLED_BOSS_WAVE,
};
use crate::color::Rgb;
use crate::recipe::marks as recipe_marks;
use crate::recipe::MAX_MARKS;
use crate::waves::BossKind;
use crate::entities::Reach;
use crate::offer::{Offer, OfferItem, OFFER_ARM_TICKS, OFFER_SCORE_STEP};
use crate::config::*;
use crate::dev::{DevSetup, DEV_MAX_SCORE, DEV_MAX_WAVE, DEV_SCORE_STEP};
use crate::entities::*;
use crate::game::{Game, State};
use crate::geom::{Body, Viewport};
use crate::input::{InputFrame, PlayerIntent};
use crate::settings::{SchemeInfo, Settings};
use crate::waves::{pick_ground, GroundKind, WaveAction, WaveKind, WaveManager, WaveRule};

/// Stand-in for a desktop's scheme list: two keyboards plus a pad.
static TEST_SCHEMES: [SchemeInfo; 3] = [
    SchemeInfo { label: "KB A", is_gamepad: false, pad_index: 0 },
    SchemeInfo { label: "KB B", is_gamepad: false, pad_index: 0 },
    SchemeInfo { label: "PAD 1", is_gamepad: true, pad_index: 0 },
];

/// A platform that can only seat one player, like the PSP.
static SINGLE_SCHEME: [SchemeInfo; 1] =
    [SchemeInfo { label: "PAD", is_gamepad: true, pad_index: 0 }];

/// Colour for enemies a test builds by hand. In the game a plain enemy wears a
/// player's colour; nothing under test cares which, so this keeps the call
/// sites short and makes it obvious the value is arbitrary.
const TEST_ENEMY_COLOR: crate::color::Rgb = crate::color::Rgb::new(200.0, 200.0, 200.0);

fn new_game() -> Game {
    Game::new(
        Viewport::new(1280.0, 800.0),
        12345,
        Settings::default(),
        &TEST_SCHEMES,
        2,
    )
}

fn single_player_platform() -> Game {
    Game::new(
        Viewport::new(480.0, 272.0),
        7,
        Settings::default(),
        &SINGLE_SCHEME,
        1,
    )
}

fn idle() -> InputFrame {
    InputFrame::default()
}

/// Places a player so its melee box lands on `target`, and swings.
fn swing_at(game: &mut Game, player: usize, target: Body) {
    game.players[player].body.x = target.x - game.players[player].body.w;
    game.players[player].body.y = target.y;
    game.players[player].facing_right = true;
    let kind = game.players[player].attack;
    game.players[player].attack_ticks = kind.swing_ticks(ATTACK_TICKS);
    // The real swing gets its number from the input path; a hand-made one has
    // to take it too, or a kind that may only land once per swing never knows
    // which swing it is looking at.
    game.players[player].strike_id = game.players[player].strike_id.wrapping_add(1).max(1);
    let v = game.viewport;
    game.players[player].update_gun(&v, 1.0, game.wave);
}

#[test]
fn starts_on_title_with_one_player() {
    let game = new_game();
    assert_eq!(game.state, State::Title);
    assert_eq!(game.players.len(), 1);
    assert_eq!(game.wave, 1);
}

#[test]
fn viewport_percentages_match_the_original_scheme() {
    let v = Viewport::new(1000.0, 500.0);
    assert_eq!(v.wper(50.0), 500.0);
    assert_eq!(v.hper(10.0), 50.0);
}

#[test]
fn aabb_overlap_is_inclusive_like_the_original() {
    let a = Body::new(0.0, 0.0, 10.0, 10.0);
    let touching = Body::new(10.0, 0.0, 5.0, 5.0);
    let apart = Body::new(11.0, 0.0, 5.0, 5.0);
    assert!(a.intersects(&touching));
    assert!(!a.intersects(&apart));
}

#[test]
fn two_player_run_spawns_players_apart_with_own_colors() {
    let mut game = new_game();
    game.start_run(2);
    assert_eq!(game.players.len(), 2);
    assert_ne!(game.players[0].body.x, game.players[1].body.x);
    assert_ne!(game.players[0].color, game.players[1].color);
}

#[test]
fn each_player_scores_only_their_own_kills() {
    let mut game = new_game();
    game.start_run(2);
    // Park P2 far away so it cannot be the one connecting.
    game.players[1].body.x = game.viewport.wper(300.0);

    let mut z = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    z.hp = -1.0;
    let body = z.body;
    game.zombies.push(z);
    swing_at(&mut game, 0, body);
    game.tick(&idle());

    assert_eq!(game.players[0].score, 6, "P1 landed the blow");
    assert_eq!(game.players[1].score, 0, "P2 was nowhere near it");
    assert_eq!(game.players[0].kills, 1);
}

#[test]
fn boss_pays_boss_tier_but_runts_and_children_do_not() {
    // Regression guard: the JS keyed "is this a boss?" off hpmax == 255, so
    // runts (128) and splitter children (127) wrongly paid 100 * wave.
    let mut game = new_game();
    game.start_run(1);
    game.wave = 10;

    let reward_for = |game: &mut Game, mut z: Zombie| -> i64 {
        z.hp = -1.0;
        let body = z.body;
        game.zombies.clear();
        game.players[0].score = 0;
        game.zombies.push(z);
        swing_at(game, 0, body);
        game.tick(&idle());
        game.players[0].score
    };

    let base = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    let runt = Zombie::runt(&game.viewport, &mut game.rng);
    let boss = Zombie::boss(&game.viewport, 10, &mut game.rng);
    let parent = Zombie::splitter(&game.viewport, &mut game.rng);
    let child = Zombie::child(&parent, &game.viewport, &mut game.rng);

    assert_eq!(reward_for(&mut game, base), 6);
    assert_eq!(
        reward_for(&mut game, runt),
        6,
        "runt must not pay boss tier"
    );
    assert_eq!(
        reward_for(&mut game, child),
        6,
        "child must not pay boss tier"
    );
    assert_eq!(reward_for(&mut game, boss), 1000, "boss pays 100 * wave");
}

#[test]
fn armor_reduces_only_melee_damage() {
    let mut game = new_game();
    game.start_run(1);

    let mut plain = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    let mut armored = Zombie::armored(&game.viewport, &mut game.rng);
    plain.hp = 255.0;
    armored.hp = 255.0;

    for z in [&mut plain, &mut armored] {
        let body = z.body;
        game.zombies.clear();
        game.zombies.push(z.clone());
        swing_at(&mut game, 0, body);
        game.tick(&idle());
        z.hp = game.zombies[0].hp;
    }

    assert_eq!(plain.hp, 255.0 - 64.0);
    assert_eq!(armored.hp, 255.0 - 64.0 * ARMORED_ARMOR);
}

#[test]
fn splitter_spawns_children_that_do_not_split_again() {
    let mut game = new_game();
    game.start_run(1);
    let mut splitter = Zombie::splitter(&game.viewport, &mut game.rng);
    splitter.hp = -1.0;
    let body = splitter.body;
    let child_w = body.w / 2.0;
    game.zombies.push(splitter);
    swing_at(&mut game, 0, body);
    game.tick(&idle());

    // The wave manager may also spawn on this tick, so identify children by
    // their half size rather than by the total zombie count.
    let children: Vec<_> = game
        .zombies
        .iter()
        .filter(|z| (z.body.w - child_w).abs() < 0.01)
        .collect();
    assert_eq!(children.len(), SPLITTER_CHILD_COUNT as usize);
    assert!(
        children.iter().all(|z| z.splits_into == 0),
        "children must not split again"
    );
}

#[test]
fn no_blast_can_hurt_anyone_any_more() {
    // Blasts used to be the bomber's payload. They are decoration now: every
    // death leaves one, and standing in it costs nothing.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let mut z = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    z.hp = -1.0;
    let body = z.body;
    game.zombies.push(z);
    swing_at(&mut game, 0, body);
    game.tick(&idle());

    let blast = game
        .explosions
        .first()
        .expect("a death should still leave a blast")
        .body;

    game.players[0].body.x = blast.x;
    game.players[0].body.y = blast.y;
    game.players[0].ay = 0.0;
    game.players[0].hp = 255.0;

    for _ in 0..12 {
        game.zombies.clear();
        game.flyers.clear();
        game.tick(&idle());
    }
    assert_eq!(
        game.players[0].hp, 255.0,
        "standing in a blast should cost nothing"
    );
}

#[test]
fn a_blinker_leaves_a_blast_and_goes_when_touched() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let p = game.players[0].body;
    let mut z = Zombie::blinker(&v, &mut game.rng);
    z.body.x = p.x;
    z.body.y = p.y;
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    game.zombies.push(z);
    game.explosions.clear();
    game.flyers.clear();

    game.tick(&idle());

    assert!(!game.explosions.is_empty(), "it should leave a blast behind");
    let gap = (game.zombies[0].body.center_x() - game.players[0].body.center_x()).abs();
    assert!(
        gap > v.wper(40.0),
        "it should be back at the edge, but it is {gap} away"
    );
    assert!(game.zombies[0].hp > 0.0, "touching it should not kill it");
}

/// Puts a swing on a target that the player is *not* touching.
///
/// The attack box reaches past the body once the player has a score, which is
/// the only way to land a hit without also making contact - and contact has its
/// own rule for a blinker.
fn swing_from_range(game: &mut Game, target_x: f32, target_y: f32) {
    let v = game.viewport;
    game.players[0].score = 1000;
    game.players[0].body.x = target_x - game.players[0].body.w - v.wper(2.0);
    game.players[0].body.y = target_y;
    game.players[0].facing_right = true;
    game.players[0].attack_ticks = ATTACK_TICKS;
    game.players[0].update_gun(&v, 1.0, game.wave);
    assert!(
        !game.players[0].body.intersects(&Body::new(target_x, target_y, v.wper(5.0), v.hper(10.0))),
        "the point of this helper is that the bodies do not touch"
    );
}

#[test]
fn a_blinker_goes_on_the_first_hit_but_stands_and_fights_after() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let ground = v.hper(GROUND_Y_PCT);
    let mut z = Zombie::blinker(&v, &mut game.rng);
    z.body.y = ground - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(20.0);
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    game.zombies.push(z);
    game.flyers.clear();

    let (tx, ty) = (game.zombies[0].body.x, game.zombies[0].body.y);
    swing_from_range(&mut game, tx, ty);
    game.tick(&idle());

    assert!(game.zombies[0].hurt_once, "the hit should have registered");
    assert!(
        (game.zombies[0].body.center_x() - game.players[0].body.center_x()).abs() > v.wper(40.0),
        "the first hit should send it away"
    );

    // Bring it back within reach and hit it again: this time it takes the hit.
    game.zombies[0].body.x = game.players[0].body.x + v.wper(20.0);
    game.zombies[0].body.y = ground - game.zombies[0].body.h;
    let (tx, ty) = (game.zombies[0].body.x, game.zombies[0].body.y);
    game.players[0].attack_ticks = 0;
    game.tick(&idle());
    let hp_before = game.zombies[0].hp;

    swing_from_range(&mut game, tx, ty);
    let x_before = game.zombies[0].body.center_x();
    game.tick(&idle());

    assert!(
        game.zombies[0].hp < hp_before,
        "later hits should actually wound it"
    );
    assert!(
        (game.zombies[0].body.center_x() - x_before).abs() < v.wper(20.0),
        "only the first hit should blink it away"
    );
}

#[test]
fn shooter_fires_a_projectile_that_hurts_a_player() {
    let mut game = new_game();
    game.start_run(1);
    let mut shooter = Zombie::shooter(&game.viewport, &mut game.rng);
    shooter.behaviors.shoot = Some(0);
    shooter.body.x = 0.0;
    shooter.body.y = game.viewport.hper(75.0);
    game.zombies.push(shooter);
    game.players[0].body.x = game.viewport.wper(50.0);
    game.players[0].body.y = game.viewport.hper(75.0);

    game.tick(&idle());
    assert_eq!(
        game.projectiles.len(),
        1,
        "shooter should fire on cooldown 0"
    );

    game.players[0].hp = 255.0;
    for _ in 0..400 {
        game.tick(&idle());
        if game.players[0].hp < 255.0 {
            break;
        }
    }
    assert!(game.players[0].hp < 255.0, "projectile never connected");
}

#[test]
fn frenzied_zombie_speeds_up_only_when_wounded() {
    let mut game = new_game();
    game.start_run(1);
    let ground = game.viewport.hper(GROUND_Y_PCT);

    let mut cap_for = |wounded: bool| -> f32 {
        let mut z = Zombie::frenzied(&game.viewport, &mut game.rng);
        z.body.x = 0.0;
        z.body.y = ground - z.body.h;
        if wounded {
            z.hp = z.hpmax * 0.2;
        }
        game.zombies.clear();
        game.zombies.push(z);
        game.players[0].body.x = game.viewport.w;
        for _ in 0..40 {
            game.tick(&idle());
        }
        game.zombies[0].ax
    };

    let healthy = cap_for(false);
    let wounded = cap_for(true);
    assert!(
        wounded > healthy,
        "wounded {wounded} should out-run healthy {healthy}"
    );
}

#[test]
fn jumper_leaves_the_ground() {
    let mut game = new_game();
    game.start_run(1);
    let ground = game.viewport.hper(GROUND_Y_PCT);
    let mut jumper = Zombie::jumper(&game.viewport, &mut game.rng);
    jumper.movement = Movement::Hop { cooldown: 0 };
    jumper.body.y = ground - jumper.body.h;
    let resting_y = jumper.body.y;
    game.zombies.push(jumper);

    let mut airborne = false;
    for _ in 0..40 {
        game.tick(&idle());
        if game.zombies[0].body.y < resting_y - 1.0 {
            airborne = true;
            break;
        }
    }
    assert!(airborne, "jumper never left the ground");
}

#[test]
fn run_ends_only_when_every_player_is_down() {
    let mut game = new_game();
    game.start_run(2);

    game.players[0].hp = -1.0;
    game.players[0].dead = true;
    assert_eq!(game.state, State::Playing, "one down is not game over");
    assert_eq!(game.living_count(), 1);

    game.players[1].hp = -1.0;
    game.players[1].dead = true;
    assert_eq!(game.living_count(), 0);
}

#[test]
fn downed_player_revives_on_wave_clear_keeping_score() {
    let mut game = new_game();
    game.start_run(2);
    game.players[0].dead = true;
    game.players[0].score = 250;
    game.spawn_count = game.wave * 10;

    // Empty field + spent budget: the wave manager should clear the wave.
    for _ in 0..200 {
        game.tick(&idle());
        if !game.players[0].dead {
            break;
        }
    }

    assert!(!game.players[0].dead, "downed player should rejoin");
    assert_eq!(game.players[0].hp, REVIVE_HP);
    assert_eq!(game.players[0].score, 250, "score survives the revive");
    assert_eq!(game.wave, 2);
}

#[test]
fn difficulty_uses_the_combined_score_so_solo_is_unchanged() {
    let mut game = new_game();
    game.start_run(1);
    game.players[0].score = 500;
    assert_eq!(game.total_score(), 500);

    game.start_run(2);
    game.players[0].score = 300;
    game.players[1].score = 200;
    assert_eq!(game.total_score(), 500, "team score drives the chase ramp");
}

#[test]
fn energy_threshold_is_never_zero() {
    let mut game = new_game();
    game.start_run(1);
    game.players[0].super_charges = 0;
    game.players[0].attacks_since_power_up = 0;
    assert!(
        game.players[0].energy_needed(1) >= 1,
        "a zero threshold would blank the HUD and grant charges every tick"
    );
}

#[test]
fn spawn_table_gates_variants_by_wave() {
    let mut rng = Rng::new(99);
    let sample = |wave: i64, rng: &mut Rng| {
        let mut seen = Vec::new();
        for _ in 0..600 {
            let kind = pick_ground(wave, rng);
            if !seen.contains(&kind) {
                seen.push(kind);
            }
        }
        seen
    };

    assert_eq!(
        sample(1, &mut rng),
        vec![GroundKind::Base],
        "wave 1 is base only"
    );
    assert!(sample(2, &mut rng).contains(&GroundKind::Runt));
    assert!(!sample(2, &mut rng).contains(&GroundKind::Jumper));
    assert!(sample(9, &mut rng).contains(&GroundKind::Shooter));
    assert_eq!(
        sample(9, &mut rng).len(),
        9,
        "whole roster unlocked by wave 9"
    );
}

#[test]
fn movement_input_moves_the_player_and_stops_on_release() {
    let mut game = new_game();
    game.start_run(1);
    let start_x = game.players[0].body.x;

    let mut frame = idle();
    frame.players[0] = PlayerIntent {
        right: true,
        ..Default::default()
    };
    for _ in 0..5 {
        game.tick(&frame);
    }
    assert!(game.players[0].body.x > start_x, "should have moved right");
    assert!(game.players[0].facing_right);

    let moved_to = game.players[0].body.x;
    game.tick(&idle());
    assert_eq!(game.players[0].ax, 0.0, "release stops horizontal motion");
    assert_eq!(game.players[0].body.x, moved_to);
}

#[test]
fn pause_toggles_and_freezes_the_simulation() {
    let mut game = new_game();
    game.start_run(1);
    let mut frame = idle();
    frame.pause = true;

    game.tick(&frame);
    assert_eq!(game.state, State::Paused);

    let timer_when_paused = game.timer;
    game.tick(&idle());
    assert_eq!(
        game.timer, timer_when_paused,
        "paused game must not advance"
    );

    game.tick(&frame);
    assert_eq!(game.state, State::Playing);
}

#[test]
fn settings_cycling_never_lets_players_share_a_scheme_or_colour() {
    let mut game = new_game();
    for _ in 0..12 {
        game.settings.cycle(0, true, 1, game.schemes.len());
        assert_ne!(
            game.settings.players[0].scheme,
            game.settings.players[1].scheme
        );
        game.settings.cycle(0, false, 1, game.schemes.len());
        assert_ne!(
            game.settings.players[0].color_index,
            game.settings.players[1].color_index
        );
    }
}

#[test]
fn waves_progress_and_enemies_actually_spawn() {
    let mut game = new_game();
    game.start_run(1);
    game.players[0].hp = 1.0e9;

    let mut spawned_any = false;
    for _ in 0..20_000 {
        game.tick(&idle());
        if !game.zombies.is_empty() || !game.flyers.is_empty() {
            spawned_any = true;
            // Clear the field so the wave can complete.
            game.zombies.clear();
            game.flyers.clear();
        }
        game.players[0].hp = 1.0e9;
        if game.wave >= 3 {
            break;
        }
    }
    assert!(spawned_any, "no enemies ever spawned");
    assert!(
        game.wave >= 3,
        "waves did not progress, stuck at {}",
        game.wave
    );
}

#[test]
fn single_controller_platform_hides_two_player_mode() {
    let mut game = single_player_platform();
    let rows = game.menu_rows([false; MAX_PLAYERS]);
    let labels: Vec<_> = rows.iter().map(|r| r.label.as_str()).collect();
    assert!(labels.contains(&"1 PLAYER"));
    assert!(
        !labels.contains(&"2 PLAYERS"),
        "single-pad hardware must not offer co-op: {labels:?}"
    );

    // Even if something asks for two, the cap holds.
    game.start_run(2);
    assert_eq!(game.players.len(), 1);
}

#[test]
fn records_raise_the_dirty_flag_for_the_frontend_to_persist() {
    let mut settings = Settings::default();
    assert!(!settings.dirty);
    settings.set_record(1, 500);
    assert!(settings.dirty, "frontend needs to know it must save");
    assert_eq!(settings.record(1), 500);
}

#[test]
fn sanitize_repairs_a_corrupt_settings_file() {
    let mut settings = Settings::default();
    settings.players[0].scheme = 99;
    settings.players[1].color_index = 99;
    settings.players[0].color_index = 3;
    settings.players[1].scheme = 0;
    settings.record_solo = -5;
    settings.sanitize(TEST_SCHEMES.len());

    assert!(settings.players[0].scheme < TEST_SCHEMES.len());
    assert!(settings.players[1].color_index < PLAYER_COLORS.len());
    assert_ne!(settings.players[0].color_index, settings.players[1].color_index);
    assert_eq!(settings.record_solo, 0);
}

#[test]
fn an_ordinary_landing_is_silent_but_the_wall_slam_is_not() {
    use crate::audio::AudioEvent;

    let mut game = new_game();
    game.start_run(1);

    let drain = |g: &mut Game| -> Vec<AudioEvent> { g.audio.drain().collect() };

    // Plain jump, then fall back down: the touchdown itself makes no sound.
    let mut input = InputFrame::default();
    input.players[0].jump = true;
    game.tick(&input);
    drain(&mut game);

    let mut fired = Vec::new();
    for _ in 0..200 {
        game.tick(&InputFrame::default());
        fired.extend(drain(&mut game));
        if game.players[0].grounded {
            break;
        }
    }
    assert!(
        game.players[0].grounded,
        "the player should have come back down"
    );
    assert!(
        !fired.contains(&AudioEvent::Slam),
        "a plain landing must stay silent, got {fired:?}"
    );

    // Now jump and slam, which puts up the wall - that is what should sound.
    let charges_before = game.players[0].super_charges;
    assert!(charges_before > 0, "test needs a charge to spend");

    let mut jump = InputFrame::default();
    jump.players[0].jump = true;
    game.tick(&jump);
    drain(&mut game);

    // The slam can land on the very tick it is pressed, so collect from there on.
    let mut slam = InputFrame::default();
    slam.players[0].slam = true;
    game.tick(&slam);
    let mut fired = drain(&mut game);

    for _ in 0..200 {
        game.tick(&InputFrame::default());
        fired.extend(drain(&mut game));
        if game.players[0].grounded {
            break;
        }
    }
    assert!(
        fired.contains(&AudioEvent::Slam),
        "the wall effect should sound, got {fired:?}"
    );
    assert_eq!(
        game.players[0].super_charges,
        charges_before - 1,
        "the wall should have cost a charge"
    );
}

#[test]
fn a_restricted_wave_spawns_only_its_own_kind() {
    use crate::waves::{WaveAction, WaveKind};

    // Flyers-only must never ask for a ground enemy, and vice versa.
    for (kind, expect_flyer) in [(WaveKind::FlyersOnly, true), (WaveKind::GroundOnly, false)] {
        let mut game = new_game();
        game.start_run(1);
        game.waves.kind = kind;
        game.waves.begin_countdown(0);

        let mut ground = 0;
        let mut flyers = 0;
        for _ in 0..4000 {
            let live = game.zombies.len() + game.flyers.len();
            match game.waves.update(3, 0, live, &mut game.rng) {
                WaveAction::SpawnGround(_) => ground += 1,
                WaveAction::SpawnFlyer(_) => flyers += 1,
                _ => {}
            }
        }
        if expect_flyer {
            assert!(flyers > 0, "{kind:?} should have spawned something");
            assert_eq!(ground, 0, "{kind:?} let a ground enemy through");
        } else {
            assert!(ground > 0, "{kind:?} should have spawned something");
            assert_eq!(flyers, 0, "{kind:?} let a flyer through");
        }
    }
}

#[test]
fn a_jumpers_wave_ignores_the_usual_unlock_wave() {
    use crate::waves::{GroundKind, WaveAction, WaveKind};

    // Jumpers normally unlock at wave 3; a jumpers-only wave overrides that.
    let mut game = new_game();
    game.start_run(1);
    game.waves.kind = WaveKind::JumpersOnly;
    game.waves.begin_countdown(0);

    let mut seen = 0;
    for _ in 0..4000 {
        if let WaveAction::SpawnGround(kind) = game.waves.update(1, 0, 0, &mut game.rng) {
            assert!(
                matches!(kind, GroundKind::Jumper | GroundKind::Leaper),
                "a jumpers wave spawned {kind:?}"
            );
            seen += 1;
        }
    }
    assert!(seen > 0, "jumpers wave never spawned at wave 1");
}

#[test]
fn special_waves_are_roughly_one_in_four_and_never_on_wave_one_or_a_boss() {
    use crate::waves::WaveKind;

    let mut rng = Rng::new(0xA11CE);
    let mut special = 0;
    let mut rolls = 0;
    for wave in 2..2000i64 {
        if wave % 5 == 0 {
            assert_eq!(
                WaveKind::roll(wave, &mut rng),
                WaveKind::Mixed,
                "boss wave {wave} must stay mixed"
            );
            continue;
        }
        rolls += 1;
        if WaveKind::roll(wave, &mut rng) != WaveKind::Mixed {
            special += 1;
        }
    }
    let share = special as f32 / rolls as f32;
    assert!(
        (0.20..0.30).contains(&share),
        "special waves should land near 25%, got {:.1}%",
        share * 100.0
    );

    for _ in 0..100 {
        assert_eq!(WaveKind::roll(1, &mut rng), WaveKind::Mixed, "wave 1 is the introduction");
    }
}

#[test]
fn death_shows_a_result_screen_that_freezes_everything_then_returns_to_title() {
    let mut game = new_game();
    game.start_run(1);
    game.players[0].score = 4200;
    game.spawn_count = 5;
    game.waves.begin_countdown(0);

    // Give the wave something to freeze, then kill the only player.
    for _ in 0..600 {
        game.tick(&InputFrame::default());
        if !game.zombies.is_empty() || !game.flyers.is_empty() {
            break;
        }
    }
    assert!(!game.zombies.is_empty(), "test needs a live enemy to freeze");

    game.players[0].hp = 1.0;
    let idx = 0;
    while game.state == State::Playing {
        game.players[idx].hp = -1.0;
        game.tick(&InputFrame::default());
    }
    assert_eq!(game.state, State::GameOver, "death should open the result screen");

    // Snapshot once frozen, not before: the run is still simulating up to death.
    let enemies_before: Vec<_> = game.zombies.iter().map(|z| (z.body.x, z.body.y)).collect();
    assert!(!enemies_before.is_empty(), "the enemies should still be on screen");

    let result = game.result.expect("a result should be recorded");
    assert_eq!(result.score, 4200);
    assert!(result.is_record, "4200 beats the default record of 0");

    // Nothing moves, and a button held through the death is refused.
    let mut hammering = InputFrame::default();
    hammering.menu.confirm = true;
    hammering.pause = true;
    hammering.players[0].jump = true;
    for _ in 0..GAME_OVER_TICKS {
        assert!(!game.awaiting_dismiss(), "the prompt is early");
        game.tick(&hammering);
        assert_eq!(game.state, State::GameOver, "the hold must not be skippable");
    }
    let enemies_after: Vec<_> = game.zombies.iter().map(|z| (z.body.x, z.body.y)).collect();
    assert_eq!(enemies_before, enemies_after, "enemies should be frozen");

    // The hold is over: the prompt is up and the screen waits, however long.
    assert!(game.awaiting_dismiss(), "the prompt should be showing now");
    for _ in 0..600 {
        game.tick(&InputFrame::default());
        assert_eq!(game.state, State::GameOver, "it should wait for a key, not time out");
    }

    // A key press returns to the menu.
    let mut confirm = InputFrame::default();
    confirm.menu.confirm = true;
    game.tick(&confirm);
    assert_eq!(game.state, State::Title);
    assert!(game.result.is_none());
    assert_eq!(game.settings.record(1), 4200, "the record should have been kept");
}

#[test]
fn exit_is_offered_and_only_asks_the_frontend_to_quit() {
    let mut game = new_game();
    let rows = game.menu_rows([false; MAX_PLAYERS]);
    let exit = rows
        .iter()
        .position(|r| r.label == "EXIT")
        .expect("the title menu should offer EXIT");

    assert!(!game.quit_requested);
    game.title_menu.index = exit;
    let mut confirm = InputFrame::default();
    confirm.menu.confirm = true;
    game.tick(&confirm);

    assert!(game.quit_requested, "EXIT should raise the request");
    // The core does not shut anything down itself.
    assert_eq!(game.state, State::Title);
}

#[test]
fn psp_settings_survive_a_round_trip_through_the_dirty_flag() {
    // The PSP writes settings only when the core says they changed; if the flag
    // is never raised, records are silently lost between sessions.
    let mut game = new_game();
    game.players[0].score = 900;
    game.settings.dirty = false;
    game.start_run(1);

    let count = game.players.len();
    game.settings.set_record(count, 900);
    assert!(game.settings.dirty, "a new record must ask to be saved");
}

/// Drops the player onto the floor with the arena kept clear, so a test is not
/// fighting the wave manager while it sets up.
fn settle_on_floor(game: &mut Game) {
    for _ in 0..400 {
        clear_arena(game);
        game.tick(&idle());
        if game.players[0].grounded {
            return;
        }
    }
    panic!("player never reached the floor");
}

fn clear_arena(game: &mut Game) {
    game.zombies.clear();
    game.flyers.clear();
    game.projectiles.clear();
    game.explosions.clear();
}

/// Puts a zombie against the player from the given side and lets the hit land.
/// Returns where the player was at the moment of impact.
fn hit_player_from(game: &mut Game, from_right: bool) -> f32 {
    let px = game.players[0].body.x;
    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.x = if from_right { px + v.wper(2.0) } else { px - v.wper(2.0) };
    z.body.y = game.players[0].body.y;
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    game.zombies.push(z);
    game.tick(&idle());
    px
}

#[test]
fn a_hit_throws_the_player_up_and_away_from_the_attacker() {
    for from_right in [true, false] {
        let mut game = new_game();
        game.start_run(1);
        settle_on_floor(&mut game);

        let before = hit_player_from(&mut game, from_right);
        assert!(
            game.players[0].ay < 0.0,
            "the hit should throw the player upward, ay = {}",
            game.players[0].ay
        );

        let push = game.players[0].knockback_x;
        if from_right {
            assert!(push < 0.0, "hit from the right should push left, got {push}");
        } else {
            assert!(push > 0.0, "hit from the left should push right, got {push}");
        }

        // And the player actually travels that way over the next few ticks.
        for _ in 0..10 {
            clear_arena(&mut game);
            game.tick(&idle());
        }
        let moved = game.players[0].body.x - before;
        if from_right {
            assert!(moved < 0.0, "player should have moved left, moved {moved}");
        } else {
            assert!(moved > 0.0, "player should have moved right, moved {moved}");
        }
    }
}

#[test]
fn the_push_stops_dead_on_landing_and_does_not_slide() {
    // The original kept the sideways velocity after touchdown, so a hit sent the
    // player skating along the floor. Landing has to end the throw.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    hit_player_from(&mut game, true);
    assert!(game.players[0].knockback_x < 0.0, "the hit should have pushed");

    // The hit resolves after the player's own update, so this tick still has
    // them flagged as standing. Let them actually leave the floor first.
    let mut lifted = false;
    for _ in 0..10 {
        clear_arena(&mut game);
        game.tick(&idle());
        if !game.players[0].grounded {
            lifted = true;
            break;
        }
    }
    assert!(lifted, "the hit should have taken the player off the ground");

    let mut ticks = 0;
    while !game.players[0].grounded && ticks < 600 {
        clear_arena(&mut game);
        game.tick(&idle());
        ticks += 1;
    }
    assert!(game.players[0].grounded, "player never landed");
    assert_eq!(game.players[0].knockback_x, 0.0, "landing must clear the push");

    let resting = game.players[0].body.x;
    for _ in 0..120 {
        clear_arena(&mut game);
        game.tick(&idle());
    }
    assert_eq!(
        game.players[0].body.x, resting,
        "the player slid along the ground after landing"
    );
}

#[test]
fn being_hit_does_not_take_away_the_controls() {
    // Knockback lives on its own velocity, so walking has to keep working.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    hit_player_from(&mut game, true);

    let mut right = idle();
    right.players[0] = PlayerIntent {
        right: true,
        ..Default::default()
    };
    game.tick(&right);
    assert!(
        game.players[0].ax > 0.0,
        "input should still drive the player while they are being thrown"
    );
}

#[test]
fn projectiles_knock_back_too() {
    // Projectiles used to deal damage without moving the player at all. Blasts
    // were in this test as well, until they stopped doing damage entirely.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let p = game.players[0].body;
    let v = game.viewport;
    game.projectiles.push(Projectile {
        body: Body::new(p.x + p.w + 1.0, p.y, v.wper(1.5), v.hper(1.5)),
        ax: -v.wper(1.0),
        ay: 0.0,
        damage: 8.0,
        dead: false,
    });
    game.tick(&idle());
    assert!(
        game.players[0].knockback_x < 0.0,
        "a projectile from the right should push the player left"
    );
    assert!(game.players[0].ay < 0.0, "and lift them off the ground");
}

#[test]
fn volume_rows_step_by_ten_and_clamp_at_both_ends() {
    use crate::settings::{VolumeChannel, VOLUME_MAX, VOLUME_STEP};

    let mut settings = Settings::default();
    assert_eq!(settings.music_volume, VOLUME_MAX, "sound is on by default");

    settings.adjust_volume(VolumeChannel::Music, -1);
    assert_eq!(settings.music_volume, VOLUME_MAX - VOLUME_STEP);
    assert!(settings.dirty, "a change must ask to be saved");

    // All the way down, then further: it stops at silence rather than wrapping
    // round to full blast.
    for _ in 0..40 {
        settings.adjust_volume(VolumeChannel::Music, -1);
    }
    assert_eq!(settings.music_volume, 0);
    for _ in 0..40 {
        settings.adjust_volume(VolumeChannel::Music, 1);
    }
    assert_eq!(settings.music_volume, VOLUME_MAX);

    // The two channels are independent.
    settings.adjust_volume(VolumeChannel::Sfx, -1);
    assert_eq!(settings.sfx_volume, VOLUME_MAX - VOLUME_STEP);
    assert_eq!(settings.music_volume, VOLUME_MAX);
}

#[test]
fn a_hand_edited_volume_is_snapped_back_onto_the_step() {
    let mut settings = Settings {
        music_volume: 63,
        sfx_volume: 999,
        ..Settings::default()
    };
    settings.sanitize(2);
    assert_eq!(settings.music_volume, 60, "63 should snap to the nearest notch");
    assert_eq!(settings.sfx_volume, 100, "anything over full is full");
}

#[test]
fn the_settings_screen_offers_both_volume_rows_and_they_adjust() {
    use crate::menu::MenuAction;
    use crate::settings::VolumeChannel;

    let mut game = new_game();
    game.state = State::Settings;
    let rows = game.menu_rows([false; MAX_PLAYERS]);

    let music = rows
        .iter()
        .position(|r| r.action == MenuAction::AdjustVolume(VolumeChannel::Music))
        .expect("settings should offer a music row");
    assert!(rows[music].label.starts_with("MUSIC: "), "{}", rows[music].label);
    assert!(rows[music].is_adjustable(), "it should be a left/right row");
    assert!(rows
        .iter()
        .any(|r| r.action == MenuAction::AdjustVolume(VolumeChannel::Sfx)));

    // Left on that row turns the music down.
    game.settings_menu.index = music;
    let mut left = idle();
    left.menu.left = true;
    game.tick(&left);
    assert_eq!(game.settings.music_volume, 90);
    assert!(game.menu_rows([false; MAX_PLAYERS])[music].label.ends_with("90"));
}

#[test]
fn the_camera_follows_the_player_and_the_field_has_no_walls() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let mut right = idle();
    right.players[0] = PlayerIntent { right: true, ..Default::default() };

    // Walk far past where the old right-hand wall used to be.
    for _ in 0..400 {
        clear_arena(&mut game);
        game.tick(&right);
    }
    let x = game.players[0].body.x;
    assert!(
        x > game.viewport.w * 2.0,
        "the player should have walked well past one screen, at {x}"
    );

    // And the view came with them: the player is still on screen.
    let on_screen = x - game.camera_x;
    assert!(
        on_screen > 0.0 && on_screen < game.viewport.w,
        "player left the view, screen x = {on_screen}"
    );
}

#[test]
fn enemies_spawn_into_view_however_far_the_player_has_walked() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    // Teleport the run a long way down the field.
    let far = game.viewport.w * 25.0;
    game.players[0].body.x = far;
    for _ in 0..200 {
        game.tick(&idle());
    }

    assert!(!game.zombies.is_empty(), "enemies should still be arriving");
    for z in game.zombies.iter() {
        let from_player = (z.body.center_x() - game.players[0].body.center_x()).abs();
        assert!(
            from_player < game.viewport.w * 3.0,
            "an enemy spawned {from_player} away, nowhere near the player"
        );
    }
}

#[test]
fn a_straggler_is_brought_back_rather_than_left_to_stall_the_wave() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let mut z = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.x = game.players[0].body.x + game.viewport.w * 40.0;
    game.zombies.push(z);
    game.tick(&idle());

    let dx = (game.zombies.last().unwrap().body.center_x()
        - game.players[0].body.center_x())
    .abs();
    assert!(
        dx < game.viewport.w * ENEMY_RECYCLE_SCREENS,
        "the straggler was left {dx} away instead of being recycled"
    );
}

#[test]
fn flyers_are_held_inside_the_view() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let size_ref = game.players[0].body;
    let timer = game.timer;
    let mut f = Flyer::from_edge(&game.viewport, &size_ref, timer, &mut game.rng);
    f.body.x = game.camera_x + game.viewport.w / 2.0;
    game.flyers.push(f);

    let mut right = idle();
    right.players[0] = PlayerIntent { right: true, ..Default::default() };
    for _ in 0..300 {
        game.zombies.clear();
        game.tick(&right);
        for fl in game.flyers.iter() {
            let sx = fl.body.x - game.camera_x;
            assert!(
                sx >= -1.0 && sx + fl.body.w <= game.viewport.w + 1.0,
                "a flyer escaped the view at screen x = {sx}"
            );
        }
    }
    assert!(!game.flyers.is_empty(), "the flyer should still be around");
}

#[test]
fn co_op_players_cannot_walk_off_each_others_screen() {
    let mut game = new_game();
    game.start_run(2);
    for _ in 0..200 {
        clear_arena(&mut game);
        game.tick(&idle());
    }

    // One runs left, the other right, for a long time.
    let mut apart = idle();
    apart.players[0] = PlayerIntent { left: true, ..Default::default() };
    apart.players[1] = PlayerIntent { right: true, ..Default::default() };
    for _ in 0..600 {
        clear_arena(&mut game);
        game.tick(&apart);
    }

    let span = (game.players[0].body.center_x() - game.players[1].body.center_x()).abs();
    assert!(
        span <= game.viewport.wper(PLAYER_LEASH_PCT) + 2.0,
        "the pair drifted {span} apart, more than one screen holds"
    );
    for p in game.players.iter() {
        let sx = p.body.center_x() - game.camera_x;
        assert!(
            sx > 0.0 && sx < game.viewport.w,
            "a player ended up off screen at {sx}"
        );
    }
}

#[test]
fn the_skyline_is_deterministic_and_scrolls_slower_than_the_world() {
    use crate::backdrop::visible_blocks;

    // The farthest layer: the one the parallax figure below is about.
    const FAR: f32 = BACKDROP_LAYERS[0].0;

    let v = Viewport::new(1280.0, 800.0);
    let (mut a, mut b) = (Vec::new(), Vec::new());

    // Same camera, same seed, same skyline - twice.
    visible_blocks(4321.0, &v, 99, FAR, &mut a);
    visible_blocks(4321.0, &v, 99, FAR, &mut b);
    assert_eq!(a, b, "the same spot must draw the same blocks");
    assert!(!a.is_empty(), "something should be visible");

    // Walking away and back returns the identical skyline: nothing is stored,
    // so this is what proves the hashing works.
    visible_blocks(0.0, &v, 99, FAR, &mut a);
    visible_blocks(50_000.0, &v, 99, FAR, &mut b);
    visible_blocks(0.0, &v, 99, FAR, &mut b);
    assert_eq!(a, b, "coming back should look the same as leaving");

    // A different seed gives a different skyline.
    visible_blocks(0.0, &v, 7, FAR, &mut b);
    assert_ne!(a, b, "the seed should change the layout");

    // Blocks obey the sizes asked for, and stand on the ground.
    let ground = v.hper(GROUND_Y_PCT);
    for block in a.iter() {
        assert!(block.w >= v.wper(BACKDROP_MIN_W_PCT) - 0.01);
        assert!(block.w <= v.wper(BACKDROP_MAX_W_PCT) + 0.01);
        assert!(block.h >= v.hper(BACKDROP_MIN_H_PCT) - 0.01);
        assert!(block.h <= v.hper(BACKDROP_MAX_H_PCT) + 0.01);
        assert!((block.y + block.h - ground).abs() < 0.01, "block is not on the ground");
    }

    // Parallax: moving the camera one screen must shift the skyline by a
    // quarter of that, not the whole thing.
    visible_blocks(0.0, &v, 5, FAR, &mut a);
    visible_blocks(v.w, &v, 5, FAR, &mut b);
    let moved = a[0].x - b[0].x;
    let expected = v.w * FAR;
    assert!(
        (moved - expected).abs() < v.w * 0.35,
        "skyline shifted {moved}, expected around {expected}"
    );
}

#[test]
fn the_skyline_takes_its_colour_from_the_live_background() {
    let game = new_game();
    let bg = game.background.to_rgb();
    let mut blocks = Vec::new();

    let mut previous = f32::MAX;
    for (layer, (_, want_shade)) in BACKDROP_LAYERS.iter().enumerate() {
        let shade = game.backdrop_layer(layer, &mut blocks);
        assert!(shade.g < bg.g, "every layer must be darker than the background");
        let want = libm::floorf(bg.g * want_shade);
        assert!((shade.g - want).abs() < 1.5, "layer {layer} has the wrong shade");
        // Nearer layers are darker, which is what reads as depth.
        assert!(shade.g < previous, "layer {layer} is not darker than the one behind");
        previous = shade.g;
    }
}

#[test]
fn backdrop_fill_cost_on_a_psp_screen() {
    use crate::backdrop::visible_blocks;

    let v = Viewport::new(480.0, 272.0);
    let screen = (v.w * v.h) as f64;
    let mut out = Vec::new();

    let (mut worst_blocks, mut worst_area) = (0usize, 0.0f64);
    let (mut sum_blocks, mut sum_area, mut samples) = (0usize, 0.0f64, 0usize);

    // Walk a long way, sampling every few pixels of camera travel.
    for step in 0..4000 {
        let cam = step as f32 * 7.0;
        let (mut blocks, mut area) = (0usize, 0.0f64);
        for (i, (parallax, _)) in BACKDROP_LAYERS.iter().enumerate() {
            let seed = 12345u64 ^ (i as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
            visible_blocks(cam, &v, seed, *parallax, &mut out);
            blocks += out.len();
            for b in out.iter() {
                // Only the part actually on screen costs anything.
                let x0 = b.x.max(0.0);
                let x1 = (b.x + b.w).min(v.w);
                let y0 = b.y.max(v.hper(CEILING_H_PCT));
                let y1 = (b.y + b.h).min(v.hper(GROUND_Y_PCT));
                if x1 > x0 && y1 > y0 {
                    area += ((x1 - x0) * (y1 - y0)) as f64;
                }
            }
        }
        if blocks > worst_blocks { worst_blocks = blocks; }
        if area > worst_area { worst_area = area; }
        sum_blocks += blocks; sum_area += area; samples += 1;
    }

    // Per-layer, so the cost of adding layers is visible rather than inferred.
    for (i, (parallax, _)) in BACKDROP_LAYERS.iter().enumerate() {
        let seed = 12345u64 ^ (i as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        let (mut b_sum, mut a_sum) = (0usize, 0.0f64);
        for step in 0..4000 {
            visible_blocks(step as f32 * 7.0, &v, seed, *parallax, &mut out);
            b_sum += out.len();
            for b in out.iter() {
                let x0 = b.x.max(0.0);
                let x1 = (b.x + b.w).min(v.w);
                let y0 = b.y.max(v.hper(CEILING_H_PCT));
                let y1 = (b.y + b.h).min(v.hper(GROUND_Y_PCT));
                if x1 > x0 && y1 > y0 { a_sum += ((x1 - x0) * (y1 - y0)) as f64; }
            }
        }
        std::eprintln!(
            "  layer {i} (parallax {parallax}): {:.1} blocks, {:.2} screens of fill",
            b_sum as f64 / 4000.0, a_sum / 4000.0 / screen
        );
    }

    let avg_blocks = sum_blocks as f64 / samples as f64;
    let avg_area = sum_area / samples as f64;
    std::eprintln!(
        "layers={}  blocks/frame avg {:.1} worst {}  fill avg {:.2} worst {:.2} screens",
        BACKDROP_LAYERS.len(), avg_blocks, worst_blocks,
        avg_area / screen, worst_area / screen
    );
    std::eprintln!(
        "  at 60 fps: {:.1} Mpixel/s worst case",
        worst_area * 60.0 / 1e6
    );
    assert!(worst_blocks < 64, "far more blocks than a PSP frame should carry");
}

#[test]
fn ground_enemies_follow_the_player_however_far_out_they_go() {
    // The arena used to pin zombies between -50% and 150% of one screen. Those
    // were absolute world coordinates, so on an endless field the chase stopped
    // dead past 1.5 screens - and with straggler recycling pulling them back,
    // they flickered between the player and that wall every tick.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let far = v.w * 30.0;
    game.players[0].body.x = far;
    game.camera_x = far - v.w / 2.0;

    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.x = far - v.wper(40.0);
    z.body.y = game.players[0].body.y;
    game.zombies.push(z);

    let mut closest = f32::MAX;
    let mut jumps = 0;
    let mut last = game.zombies[0].body.x;
    for _ in 0..240 {
        game.flyers.clear();
        game.projectiles.clear();
        game.tick(&idle());
        let Some(z) = game.zombies.first() else { break };
        let gap = (z.body.center_x() - game.players[0].body.center_x()).abs();
        closest = closest.min(gap);
        // A tick should never move an enemy further than its own speed; a
        // teleport back to an old boundary would blow straight past that.
        if (z.body.x - last).abs() > v.wper(10.0) {
            jumps += 1;
        }
        last = z.body.x;
    }

    assert_eq!(jumps, 0, "the enemy was being snapped across the field");
    assert!(
        closest < v.wper(20.0),
        "the enemy never closed on a player standing 30 screens out, best gap {closest}"
    );
}

#[test]
fn wave_pacing_carries_no_assumptions_about_where_the_run_is() {
    // Weaker than it looks: this clears the enemies itself, so it does not
    // exercise the chase. What it does cover is that the wave manager and the
    // clear-up path work on coordinates far from zero.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    game.players[0].body.x = v.w * 18.0;
    game.camera_x = game.players[0].body.x - v.w / 2.0;

    // Spend the wave's budget, then let the stragglers be dealt with.
    game.spawn_count = game.wave * 10;
    let started_on = game.wave;
    for _ in 0..4000 {
        game.zombies.clear();
        game.flyers.clear();
        game.tick(&idle());
        if game.wave > started_on {
            break;
        }
    }
    assert!(
        game.wave > started_on,
        "the wave never cleared while the run was far from the origin"
    );
}

#[test]
fn leaving_a_run_from_pause_has_to_be_confirmed() {
    use crate::menu::MenuAction;

    let mut game = new_game();
    game.start_run(1);
    game.players[0].score = 900;

    let mut pause = idle();
    pause.pause = true;
    game.tick(&pause);
    assert_eq!(game.state, State::Paused);

    let rows = game.menu_rows([false; MAX_PLAYERS]);
    assert_eq!(rows[0].action, MenuAction::Resume);
    assert_eq!(rows[1].action, MenuAction::AskAbandon, "pause needs a way out");

    // Pick EXIT: that only asks the question.
    let mut down = idle();
    down.menu.down = true;
    game.tick(&down);
    let mut confirm = idle();
    confirm.menu.confirm = true;
    game.tick(&confirm);
    assert_eq!(game.state, State::ConfirmAbandon, "it must ask first");

    // "No" is preselected, so a mashed button keeps the run.
    assert_eq!(game.menu_rows([false; MAX_PLAYERS])[0].action, MenuAction::KeepPlaying);
    game.tick(&confirm);
    assert_eq!(game.state, State::Paused, "answering no returns to the pause menu");

    // Now really leave.
    game.tick(&down);
    game.tick(&confirm);
    assert_eq!(game.state, State::ConfirmAbandon);
    game.tick(&down);
    game.tick(&confirm);
    assert_eq!(game.state, State::Title, "yes should end the run");
    assert_eq!(game.settings.record(1), 900, "the score was earned and should stand");
}

#[test]
fn backing_out_of_the_question_keeps_the_run() {
    let mut game = new_game();
    game.start_run(1);
    let mut pause = idle();
    pause.pause = true;
    game.tick(&pause);

    let mut down = idle();
    down.menu.down = true;
    game.tick(&down);
    let mut confirm = idle();
    confirm.menu.confirm = true;
    game.tick(&confirm);
    assert_eq!(game.state, State::ConfirmAbandon);

    let mut back = idle();
    back.menu.back = true;
    game.tick(&back);
    assert_eq!(game.state, State::Paused);

    // And pause still resumes the game.
    game.tick(&pause);
    assert_eq!(game.state, State::Playing);
}

#[test]
fn a_flyers_dip_reaches_the_middle_of_a_standing_player() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let player = game.players[0].body;
    let player_middle = player.y + player.h / 2.0;

    let size_ref = player;
    let timer = game.timer;
    let mut f = Flyer::from_edge(&v, &size_ref, timer, &mut game.rng);
    f.body.x = game.camera_x + v.w / 2.0;
    f.hp = 9999.0;
    f.hpmax = 9999.0;
    game.flyers.push(f);

    // Follow one whole arc and record how low the flyer's underside gets.
    let mut lowest: f32 = f32::MIN;
    let mut highest: f32 = f32::MAX;
    for _ in 0..400 {
        game.zombies.clear();
        game.tick(&idle());
        let Some(fl) = game.flyers.first() else { break };
        lowest = lowest.max(fl.body.y + fl.body.h);
        highest = highest.min(fl.body.y);
    }

    assert!(
        (lowest - player_middle).abs() < v.hper(1.0),
        "the dip bottoms out at {lowest}, the player's middle is {player_middle}"
    );
    // And it still clears the ceiling band, or the top of the arc would vanish.
    assert!(
        highest >= v.hper(CEILING_H_PCT) - v.hper(1.0),
        "the arc rises to {highest}, behind the ceiling band"
    );
}

#[test]
fn a_teleporter_blinks_away_the_first_time_it_is_hit_and_not_after() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let size_ref = game.players[0].body;
    let timer = game.timer;
    let mut f = Flyer::teleporter(&v, &size_ref, timer, &mut game.rng);
    // Park it right on the player so the melee box reaches it.
    f.body.x = game.players[0].body.x;
    f.body.y = game.players[0].body.y;
    f.hp = 9999.0;
    f.hpmax = 9999.0;
    game.flyers.push(f);
    let before = game.flyers[0].body.x;

    let mut swing = idle();
    swing.players[0] = PlayerIntent { attack: true, ..Default::default() };
    game.zombies.clear();
    game.tick(&swing);

    assert!(game.flyers[0].hurt_once, "the hit should have registered");
    assert!(
        (game.flyers[0].body.x - before).abs() > v.wper(10.0),
        "it should have blinked away on the first hit"
    );

    // A second hit leaves it where it is: only the first one teleports.
    game.flyers[0].body.x = game.players[0].body.x;
    game.flyers[0].body.y = game.players[0].body.y;
    let second = game.flyers[0].body.x;
    game.players[0].attack_ticks = 0;
    game.tick(&idle());
    game.tick(&swing);
    assert!(
        (game.flyers[0].body.x - second).abs() < v.wper(10.0),
        "later hits should not teleport it"
    );
}

/// Runs the wave manager on a given wave and reports the boss action it picks.
fn boss_action_for(wave: i64) -> Option<WaveAction> {
    boss_action_seeded(wave, 4242)
}

fn boss_action_seeded(wave: i64, seed: u64) -> Option<WaveAction> {
    let mut waves = WaveManager::default();
    let mut rng = Rng::new(seed);
    for _ in 0..5000 {
        match waves.update(wave, 0, 0, &mut rng) {
            WaveAction::Idle
            | WaveAction::SpawnGround(_)
            | WaveAction::SpawnFlyer(_)
            | WaveAction::SpawnElite(_)
            | WaveAction::ClearWave => {}
            // Everything else *is* a boss arriving, and listing them here
            // rather than matching a few by name is what makes a new kind of
            // boss show up in this test instead of being quietly ignored.
            boss => return Some(boss),
        }
    }
    None
}

#[test]
fn each_early_boss_wave_keeps_its_own_set_piece() {
    assert_eq!(boss_action_for(5), Some(WaveAction::SpawnBosses(1)));
    assert_eq!(
        boss_action_for(FLYING_BOSS_WAVE),
        Some(WaveAction::SpawnFlyingBoss),
        "wave 10 should be the flying boss, not ground bosses"
    );
    // Wave 15 is one or the other; see the shedder-boss test below.
    assert!(matches!(
        boss_action_for(SHEDDER_BOSS_WAVE),
        Some(WaveAction::SpawnBosses(3)) | Some(WaveAction::SpawnShedderBoss)
    ));
    // Twenty used to be four ground bosses and is now the rolled boss's own
    // wave - the only one that spawns nothing else at all.
    assert_eq!(boss_action_for(ROLLED_BOSS_WAVE), Some(WaveAction::SpawnRolledBoss));
    // And a non-boss wave still brings none.
    assert_eq!(boss_action_for(7), None);
}

#[test]
fn the_flying_boss_arrives_alone_and_only_once() {
    let mut game = new_game();
    game.start_run(1);
    game.wave = FLYING_BOSS_WAVE;
    game.waves = WaveManager::default();
    game.spawn_count = 0;

    for _ in 0..3000 {
        game.tick(&idle());
    }

    let bosses = game.flyers.iter().filter(|f| f.is_boss).count();
    assert_eq!(bosses, 1, "exactly one flying boss should be about");
    assert!(
        !game.zombies.iter().any(|z| z.is_boss),
        "wave 10 should not also bring ground bosses"
    );
}

#[test]
fn the_flying_boss_is_twice_the_area_with_a_normal_flyers_health() {
    let mut game = new_game();
    game.start_run(1);
    let v = game.viewport;
    let size_ref = game.players[0].body;
    let timer = game.timer;

    let plain = Flyer::from_edge(&v, &size_ref, timer, &mut game.rng);
    let boss = Flyer::flying_boss(&v, &size_ref, timer, &mut game.rng);

    let plain_area = plain.body.w * plain.body.h;
    let boss_area = boss.body.w * boss.body.h;
    assert!(
        (boss_area / plain_area - 2.0).abs() < 0.01,
        "the boss covers {:.2}x a normal flyer, wanted 2x",
        boss_area / plain_area
    );
    assert_eq!(boss.hp, plain.hp, "same health as an ordinary flyer");
    assert_eq!(boss.hpmax, plain.hpmax);
    assert!(boss.is_boss);
}

#[test]
fn the_flying_boss_teleports_on_every_single_hit() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let size_ref = game.players[0].body;
    let timer = game.timer;
    let mut boss = Flyer::flying_boss(&v, &size_ref, timer, &mut game.rng);
    boss.hp = 99_999.0;
    boss.hpmax = 99_999.0;
    game.flyers.push(boss);

    let mut swing = idle();
    swing.players[0] = PlayerIntent { attack: true, ..Default::default() };

    // Hit it repeatedly; every single one should move it. The damage check runs
    // before the arc is recomputed, so the boss has to be placed on the player
    // in the same tick as the swing or it will have flown off by then.
    for round in 0..5 {
        game.zombies.clear();
        let p = game.players[0].body;
        game.flyers[0].body.x = p.x;
        game.flyers[0].body.y = p.y;
        game.players[0].attack_ticks = 0;
        let before = game.flyers[0].body.x;
        game.tick(&swing);
        assert!(
            (game.flyers[0].body.x - before).abs() > v.wper(10.0),
            "hit {round} did not move the boss"
        );
    }
}

#[test]
fn the_flying_boss_drives_the_boss_music_layer() {
    let mut game = new_game();
    game.start_run(1);
    assert!(!game.music_state().boss);

    let v = game.viewport;
    let size_ref = game.players[0].body;
    let timer = game.timer;
    let boss = Flyer::flying_boss(&v, &size_ref, timer, &mut game.rng);
    game.flyers.push(boss);
    assert!(
        game.music_state().boss,
        "a flying boss is still a boss as far as the music is concerned"
    );
}

#[test]
fn a_leaper_stands_still_then_lands_where_the_player_was() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let player_x = game.players[0].body.center_x();
    let mut z = Zombie::leaper(&v, &mut game.rng);
    z.body.x = player_x - v.wper(30.0);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    game.zombies.push(z);

    // It holds position for the wind-up.
    let start_x = game.zombies[0].body.x;
    for _ in 0..(LEAPER_CROUCH_TICKS - 2) {
        game.flyers.clear();
        game.tick(&idle());
        assert_eq!(
            game.zombies[0].body.x, start_x,
            "a leaper should be dead still while it winds up"
        );
    }

    // Then it launches, and the aim is taken at that moment.
    let mut aim_target = 0.0;
    let mut launched = false;
    for _ in 0..10 {
        game.flyers.clear();
        game.tick(&idle());
        if game.zombies[0].ay < 0.0 {
            aim_target = game.players[0].body.center_x();
            launched = true;
            break;
        }
    }
    assert!(launched, "the leaper never left the ground");

    // Fly the arc out.
    let ground = v.hper(GROUND_Y_PCT);
    let mut landed = false;
    for _ in 0..300 {
        game.flyers.clear();
        game.tick(&idle());
        let z = &game.zombies[0];
        if (z.body.y - (ground - z.body.h)).abs() < 0.001 && z.ay >= 0.0 {
            landed = true;
            break;
        }
    }
    assert!(landed, "the leaper never came down");

    let landed_at = game.zombies[0].body.center_x();
    assert!(
        (landed_at - aim_target).abs() < v.wper(2.0),
        "landed at {landed_at}, aimed at {aim_target}"
    );
}

#[test]
fn a_leap_is_capped_so_it_reads_as_a_jump_not_a_teleport() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let mut z = Zombie::leaper(&v, &mut game.rng);
    // Further than a leap reaches, but inside the straggler-recycling range:
    // beyond that the recycler would teleport it and the distance measured
    // here would be its doing, not the leap's.
    z.body.x = game.players[0].body.center_x() - v.w * 2.0;
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    let from = z.body.center_x();
    game.zombies.push(z);

    // Watch exactly one leap: it repeats, and totting up several would say
    // nothing about whether any single one is capped.
    let ground = v.hper(GROUND_Y_PCT);
    let mut airborne = false;
    for _ in 0..400 {
        game.flyers.clear();
        game.tick(&idle());
        if game.zombies.is_empty() {
            return;
        }
        let z = &game.zombies[0];
        let on_ground = (z.body.y - (ground - z.body.h)).abs() < 0.001;
        if !on_ground {
            airborne = true;
        } else if airborne {
            break;
        }
    }
    assert!(airborne, "the leaper never jumped");
    let travelled = game.zombies[0].body.center_x() - from;
    assert!(
        travelled <= v.wper(LEAPER_MAX_REACH_PCT) + v.wper(5.0),
        "a single leap crossed {travelled}, past its reach of {}",
        v.wper(LEAPER_MAX_REACH_PCT)
    );
}

#[test]
fn the_jumpers_wave_fields_both_kinds_of_jumper() {
    let mut waves = WaveManager::default();
    let mut rng = Rng::new(31337);
    let (mut jumpers, mut leapers, mut others) = (0, 0, 0);
    for _ in 0..4000 {
        // Force the restricted wave rather than waiting for the 25% roll.
        waves.kind = WaveKind::JumpersOnly;
        match waves.update(6, 0, 0, &mut rng) {
            WaveAction::SpawnGround(GroundKind::Jumper) => jumpers += 1,
            WaveAction::SpawnGround(GroundKind::Leaper) => leapers += 1,
            WaveAction::SpawnGround(_) | WaveAction::SpawnFlyer(_) => others += 1,
            _ => {}
        }
    }
    assert!(jumpers > 0 && leapers > 0, "both should turn up: {jumpers}/{leapers}");
    assert_eq!(others, 0, "a jumpers wave should field nothing else");
}

#[test]
fn a_shooter_shows_a_sight_that_locks_before_it_fires() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let mut z = Zombie::shooter(&v, &mut game.rng);
    z.body.x = game.players[0].body.center_x() + v.wper(30.0);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    z.behaviors.shoot = Some(SHOOTER_AIM_TICKS + 4);
    game.zombies.push(z);

    let mut saw_white = false;
    let mut saw_hot = false;
    let mut hot_ticks = 0;
    let mut fired_on = None;
    for tick in 0..(SHOOTER_AIM_TICKS + 20) {
        game.flyers.clear();
        game.projectiles.clear();
        let before = game.projectiles.len();
        game.tick(&idle());
        if !game.aim_dots.is_empty() {
            if game.aim_dots[0].hot {
                saw_hot = true;
                hot_ticks += 1;
            } else {
                saw_white = true;
            }
        }
        if game.projectiles.len() > before && fired_on.is_none() {
            fired_on = Some(tick);
            break;
        }
    }

    assert!(saw_white, "there should be a white sight before the shot");
    assert!(saw_hot, "it should go red before firing");
    assert!(fired_on.is_some(), "the shooter never fired");
    // The red stretch is the 100 ms window to get out of the way.
    assert_eq!(
        hot_ticks, SHOOTER_LOCK_TICKS,
        "the locked stretch should be {SHOOTER_LOCK_TICKS} ticks"
    );
}

#[test]
fn a_locked_sight_does_not_follow_the_player() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let mut z = Zombie::shooter(&v, &mut game.rng);
    z.body.x = game.players[0].body.center_x() + v.wper(30.0);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    // Come into the lock the way a real shooter does: aiming first.
    z.behaviors.shoot = Some(SHOOTER_AIM_TICKS);
    game.zombies.push(z);

    for _ in 0..(SHOOTER_AIM_TICKS - SHOOTER_LOCK_TICKS) {
        game.flyers.clear();
        game.tick(&idle());
    }
    let locked_at = game.zombies[0].aim.expect("the sight should be set");

    // Move the player a long way; the aim must stay where it was.
    game.players[0].body.x += v.wper(40.0);
    for _ in 0..(SHOOTER_LOCK_TICKS - 1) {
        game.flyers.clear();
        game.tick(&idle());
        if let Some(aim) = game.zombies[0].aim {
            assert_eq!(aim, locked_at, "a locked sight must not track the player");
        }
    }
}

/// Puts the run on a wave with a given rule, standing on the floor.
fn game_with_rule(rule: WaveRule) -> Game {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.waves.rule = rule;
    game
}

#[test]
fn the_two_wave_axes_are_independent() {
    // The whole point of a second enum: a wave can restrict who turns up *and*
    // change the rules, which one combined enum could never express.
    let mut rng = Rng::new(9001);
    let mut seen_both = false;
    let mut rule_counts = [0usize; 5];
    for _ in 0..4000 {
        let mut waves = WaveManager::default();
        waves.begin_wave(6, &mut rng);
        if waves.kind != WaveKind::Mixed && waves.rule != WaveRule::Normal {
            seen_both = true;
        }
        rule_counts[match waves.rule {
            WaveRule::Normal => 0,
            WaveRule::StaticCamera => 1,
            WaveRule::NoJumps => 2,
            WaveRule::NoWall => 3,
            WaveRule::Hidden => 4,
        }] += 1;
    }
    assert!(seen_both, "a wave should be able to carry one of each");
    assert!(rule_counts[0] > 2400, "most waves should stay plain");
    for (i, n) in rule_counts.iter().enumerate().skip(1) {
        assert!(*n > 0, "rule {i} never came up");
    }
    // Wave 1 is always plain, on both axes.
    let mut waves = WaveManager::default();
    waves.begin_wave(1, &mut rng);
    assert_eq!(waves.rule, WaveRule::Normal);
}

#[test]
fn a_held_wave_pins_the_view_and_walls_the_player_in() {
    let mut game = game_with_rule(WaveRule::StaticCamera);
    let v = game.viewport;
    let camera_before = game.camera_x;

    let mut right = idle();
    right.players[0] = PlayerIntent { right: true, ..Default::default() };
    for _ in 0..600 {
        clear_arena(&mut game);
        // Clearing the arena lets the wave finish, which re-rolls the rule;
        // hold it so the property under test stays in force.
        game.waves.rule = WaveRule::StaticCamera;
        game.tick(&right);
    }

    assert_eq!(game.camera_x, camera_before, "the view should not have moved");
    let x = game.players[0].body.x;
    assert!(
        x + game.players[0].body.w <= game.camera_x + v.w + 0.01,
        "the player walked out of the held view, at {x}"
    );

    // And the other way.
    let mut left = idle();
    left.players[0] = PlayerIntent { left: true, ..Default::default() };
    for _ in 0..600 {
        clear_arena(&mut game);
        game.waves.rule = WaveRule::StaticCamera;
        game.tick(&left);
    }
    assert!(game.players[0].body.x >= game.camera_x - 0.01, "walked out to the left");
    assert_eq!(game.camera_x, camera_before);
}

#[test]
fn a_held_wave_opens_on_full_health() {
    let mut game = new_game();
    game.start_run(1);
    game.players[0].hp = 20.0;

    // Clear waves until a held one comes round, then check the handover.
    //
    // The field is emptied every tick rather than once a wave: a wave ends when
    // its budget is spent *and* nothing is left standing, and a late wave keeps
    // putting rolled heavies out for as long as it owes any. Emptying it once
    // and waiting would wait forever.
    for _ in 0..400 {
        let was = game.wave;
        for _ in 0..200 {
            game.spawn_count = wave_budget(game.wave);
            game.zombies.clear();
            game.flyers.clear();
            game.tick(&idle());
            if game.wave > was {
                break;
            }
        }
        if game.waves.rule == WaveRule::StaticCamera {
            assert_eq!(
                game.players[0].hp, game.players[0].hpmax,
                "a held wave should hand back full health"
            );
            return;
        }
        game.players[0].hp = 20.0;
    }
    panic!(
        "never rolled a held wave; reached wave {} after 400 tries",
        game.wave
    );
}

#[test]
fn a_grounded_wave_swaps_the_jump_for_a_standing_wall() {
    let mut game = game_with_rule(WaveRule::NoJumps);
    let charges = game.players[0].super_charges;
    assert!(charges > 0);

    let mut jump = idle();
    jump.players[0] = PlayerIntent { jump: true, ..Default::default() };
    game.tick(&jump);
    assert!(game.players[0].grounded, "there should be no jumping at all");

    // Down puts the wall up from standing, and spends a charge.
    let mut slam = idle();
    slam.players[0] = PlayerIntent { slam: true, ..Default::default() };
    clear_arena(&mut game);
    game.tick(&slam);
    assert!(game.players[0].field.active, "down should raise the wall on the spot");
    assert_eq!(game.players[0].super_charges, charges - 1);
}

#[test]
fn a_grounded_wave_doubles_the_attack_box() {
    let plain = {
        let mut g = game_with_rule(WaveRule::Normal);
        clear_arena(&mut g);
        g.tick(&idle());
        g.players[0].gun.w
    };
    let grounded = {
        let mut g = game_with_rule(WaveRule::NoJumps);
        clear_arena(&mut g);
        g.tick(&idle());
        g.players[0].gun.w
    };
    assert!(
        (grounded / plain - 2.0).abs() < 0.01,
        "reach went from {plain} to {grounded}, wanted twice"
    );
}

/// Gets the player airborne and buys the no-wall wave's invulnerability.
fn go_invulnerable(game: &mut Game) {
    let mut jump = idle();
    jump.players[0] = PlayerIntent { jump: true, ..Default::default() };
    clear_arena(game);
    game.tick(&jump);
    for _ in 0..3 {
        clear_arena(game);
        game.tick(&idle());
    }
    assert!(!game.players[0].grounded, "the player should be in the air");

    let mut slam = idle();
    slam.players[0] = PlayerIntent { slam: true, ..Default::default() };
    clear_arena(game);
    game.tick(&slam);
}

#[test]
fn a_no_wall_wave_buys_safety_instead_of_a_wall() {
    let mut game = game_with_rule(WaveRule::NoWall);
    let charges = game.players[0].super_charges;
    go_invulnerable(&mut game);

    assert!(game.players[0].invulnerable, "it should buy invulnerability");
    assert!(!game.players[0].field.active, "and no wall this wave");
    assert_eq!(game.players[0].super_charges, charges - 1);

    // Hits land on an untouchable player for nothing. The slam drives them down
    // hard, so the zombie has to follow for the few airborne ticks that remain.
    game.players[0].hp = 100.0;
    let mut z = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    game.zombies.push(z);

    let mut hits = 0;
    let mut ticks = 0;
    while game.players[0].invulnerable && ticks < 300 {
        let p = game.players[0].body;
        game.zombies[0].body.x = p.x;
        game.zombies[0].body.y = p.y;
        let before = game.players[0].hp;
        game.flyers.clear();
        game.tick(&idle());
        if game.players[0].hp < before {
            hits += 1;
        }
        ticks += 1;
    }
    assert!(ticks > 0, "the player was never airborne and safe");
    assert_eq!(hits, 0, "an untouchable player should take nothing");
}

#[test]
fn landing_out_of_that_safety_pays_back_a_quarter() {
    // Kept apart from the damage check: with an enemy on the player, the very
    // tick that lands and heals can also carry an ordinary hit, and the two
    // would be impossible to tell apart in one assertion.
    let mut game = game_with_rule(WaveRule::NoWall);
    go_invulnerable(&mut game);
    assert!(game.players[0].invulnerable);

    game.players[0].hp = 100.0;
    let hpmax = game.players[0].hpmax;
    for _ in 0..300 {
        clear_arena(&mut game);
        game.tick(&idle());
        if !game.players[0].invulnerable {
            break;
        }
    }
    assert!(!game.players[0].invulnerable, "landing should end it");
    assert!(
        (game.players[0].hp - (100.0 + hpmax * 0.25)).abs() < 0.01,
        "landing should have healed a quarter, hp is {}",
        game.players[0].hp
    );

    // And it cannot overfill.
    game.players[0].hp = hpmax - 1.0;
    go_invulnerable(&mut game);
    for _ in 0..300 {
        clear_arena(&mut game);
        game.tick(&idle());
        if !game.players[0].invulnerable {
            break;
        }
    }
    assert!(game.players[0].hp <= hpmax, "the heal must not overfill");
}

#[test]
fn a_blind_wave_makes_the_enemies_almost_harmless() {
    let mut game = game_with_rule(WaveRule::Hidden);
    assert_eq!(game.waves.rule.damage_scale(), 0.01);

    game.players[0].hp = 200.0;
    let p = game.players[0].body;
    let mut z = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.x = p.x;
    z.body.y = p.y;
    z.hp = 9999.0;
    game.zombies.push(z);
    game.flyers.clear();
    game.tick(&idle());

    // A normal wave would have taken 16 off; here it is a hundredth of that.
    let lost = 200.0 - game.players[0].hp;
    assert!(
        lost > 0.0 && lost < 1.0,
        "a blind wave should barely scratch, but took {lost}"
    );
}

#[test]
fn every_wave_modifier_comes_up_equally_often() {
    // This caught a real bug: `Rng::range` is inclusive at both ends, so
    // `range(0, 4)` over four variants let the catch-all arm swallow two values
    // and the last variant appeared twice as often as its siblings.
    let mut rng = Rng::new(12345);
    let mut rules = [0usize; 5];
    let mut kinds = [0usize; 5];
    let n = 60000;
    for _ in 0..n {
        let mut waves = WaveManager::default();
        waves.begin_wave(7, &mut rng);
        rules[match waves.rule {
            WaveRule::Normal => 0,
            WaveRule::StaticCamera => 1,
            WaveRule::NoJumps => 2,
            WaveRule::NoWall => 3,
            WaveRule::Hidden => 4,
        }] += 1;
        kinds[match waves.kind {
            WaveKind::Mixed => 0,
            WaveKind::GroundOnly => 1,
            WaveKind::FlyersOnly => 2,
            WaveKind::BasicOnly => 3,
            WaveKind::JumpersOnly => 4,
        }] += 1;
    }

    for (name, counts) in [("rule", rules), ("kind", kinds)] {
        let special: usize = counts[1..].iter().sum();
        let rate = special as f64 / n as f64;
        assert!(
            (rate - 0.25).abs() < 0.02,
            "{name}: {rate:.3} of waves were special, wanted a quarter"
        );
        let expected = special as f64 / 4.0;
        for (i, c) in counts[1..].iter().enumerate() {
            let off = (*c as f64 - expected).abs() / expected;
            assert!(
                off < 0.15,
                "{name} variant {i} came up {c} times against {expected:.0} expected"
            );
        }
    }
}

#[test]
fn a_twenty_wave_run_almost_always_sees_a_modifier() {
    // Not a certainty - at a quarter a wave, about one run in three hundred
    // gets through twenty waves untouched. Worth pinning down so the rate
    // cannot quietly drift.
    let runs = 2000;
    let mut barren = 0;
    for seed in 1..=runs {
        let mut rng = Rng::new(seed as u64);
        let mut waves = WaveManager::default();
        let mut saw = false;
        for wave in 1..=20i64 {
            waves.begin_wave(wave, &mut rng);
            saw |= waves.rule != WaveRule::Normal;
        }
        if !saw {
            barren += 1;
        }
    }
    let rate = barren as f64 / runs as f64;
    assert!(rate < 0.02, "{rate:.3} of runs saw no modifier at all in 20 waves");
}

#[test]
fn the_pause_menu_offers_the_next_wave_only_during_the_lull() {
    use crate::menu::MenuAction;

    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    // Mid-wave there is nothing to skip.
    game.waves.skip_countdown();
    let mut pause = idle();
    pause.pause = true;
    game.tick(&pause);
    assert_eq!(game.state, State::Paused);
    let labels: Vec<_> = game
        .menu_rows([false; MAX_PLAYERS])
        .iter()
        .map(|r| r.label.clone())
        .collect();
    assert!(
        !labels.iter().any(|l| l == "START WAVE"),
        "nothing to start mid-wave: {labels:?}"
    );

    // In the lull it appears, and never first - resume stays the safe default.
    game.tick(&pause);
    game.waves.begin_countdown(10);
    game.tick(&pause);
    assert_eq!(game.state, State::Paused);
    let rows = game.menu_rows([false; MAX_PLAYERS]);
    assert_eq!(rows[0].action, MenuAction::Resume, "resume must stay first");
    assert_eq!(rows[1].action, MenuAction::StartWave);
    assert_eq!(rows[2].action, MenuAction::AskAbandon);
}

#[test]
fn starting_the_wave_from_the_pause_menu_ends_the_lull_and_resumes() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.waves.begin_countdown(10);

    let mut pause = idle();
    pause.pause = true;
    game.tick(&pause);
    assert!(game.waves.between_waves(), "the lull should still be running");

    let mut down = idle();
    down.menu.down = true;
    game.tick(&down);
    let mut confirm = idle();
    confirm.menu.confirm = true;
    game.tick(&confirm);

    assert!(!game.waves.between_waves(), "the lull should be over");
    assert_eq!(game.state, State::Playing, "and the game should be running again");

    // Enemies actually start arriving.
    let mut spawned = false;
    for _ in 0..600 {
        game.tick(&idle());
        if !game.zombies.is_empty() || !game.flyers.is_empty() {
            spawned = true;
            break;
        }
    }
    assert!(spawned, "the wave never started");
}

/// Energy banked from one clean hit on a zombie at the given health.
fn energy_from_a_hit_at(hp: f32) -> i64 {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].hp = hp;
    game.players[0].energy = 0;

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(20.0);
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    let body = z.body;
    game.zombies.push(z);
    game.flyers.clear();

    swing_at(&mut game, 0, body);
    // Landing the swing can shove the player about; restore the health being
    // tested so the award is measured at the level asked for.
    game.players[0].hp = hp;
    game.tick(&idle());
    game.players[0].energy
}

#[test]
fn a_wounded_player_banks_energy_faster() {
    let hpmax = 255.0;
    let full = energy_from_a_hit_at(hpmax);
    assert_eq!(full, 3, "full health should bank a hit at face value");

    // Linear ramp to three times as much as health runs out.
    let half = energy_from_a_hit_at(hpmax * 0.5);
    assert_eq!(half, 6, "half health should double it");

    let brink = energy_from_a_hit_at(hpmax * 0.01);
    assert_eq!(brink, 9, "on the brink it should be tripled");

    // Monotonic all the way down: no step to sit on.
    let mut previous = 0;
    for step in 0..=10 {
        let hp = hpmax * (10 - step) as f32 / 10.0;
        let banked = energy_from_a_hit_at(hp.max(0.01));
        assert!(banked >= previous, "energy should never fall as health does");
        previous = banked;
    }
}

#[test]
fn getting_hurt_does_not_pay_in_score() {
    // Only the wall charges come faster; the scoreboard stays honest.
    let score_at = |hp: f32| {
        let mut game = new_game();
        game.start_run(1);
        settle_on_floor(&mut game);
        game.players[0].hp = hp;
        game.players[0].score = 0;

        let v = game.viewport;
        let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
        z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
        z.body.x = game.players[0].body.x + v.wper(20.0);
        z.hp = 9999.0;
        z.hpmax = 9999.0;
        let body = z.body;
        game.zombies.push(z);
        game.flyers.clear();
        swing_at(&mut game, 0, body);
        game.players[0].hp = hp;
        game.tick(&idle());
        game.players[0].score
    };
    assert_eq!(score_at(255.0), score_at(2.55), "score must not scale with damage taken");
}

#[test]
fn the_launch_counter_survives_a_save_and_seeds_a_fresh_run() {
    // It exists because a PSP emulator reports the same uptime every launch, so
    // the clock alone left every run identical. If this ever stops persisting,
    // the seed stops moving and nobody would notice until the waves repeated.
    let mut settings = Settings::default();
    assert_eq!(settings.launches, 0);

    settings.launches = settings.launches.wrapping_add(1);
    settings.dirty = true;
    assert_eq!(settings.launches, 1);

    // Sanitising must leave it alone; it is not a player-facing setting.
    settings.launches = 12345;
    settings.sanitize(2);
    assert_eq!(settings.launches, 12345, "sanitize must not touch the counter");

    // And it must survive whatever a frontend does to the rest.
    let mut copy = settings.clone();
    copy.music_volume = 40;
    copy.sanitize(2);
    assert_eq!(copy.launches, 12345);
}

#[test]
fn the_soundtrack_is_redrawn_at_the_menu_and_at_every_run() {
    let mut game = new_game();
    let at_start = game.audio_roll;

    // Starting a run draws one.
    game.start_run(1);
    let in_run = game.audio_roll;
    assert_ne!(in_run, at_start, "starting a run should redraw the soundtrack");

    // It stays put while the run is on.
    for _ in 0..120 {
        game.tick(&idle());
    }
    assert_eq!(game.audio_roll, in_run, "it must not change mid-run");

    // Coming back to the menu draws another, however it was arrived at.
    game.state = State::Title;
    game.tick(&idle());
    let at_menu = game.audio_roll;
    assert_ne!(at_menu, in_run, "reaching the menu should redraw it");

    // Sitting in the menu leaves it alone.
    for _ in 0..120 {
        game.tick(&idle());
    }
    assert_eq!(game.audio_roll, at_menu, "it must not churn while idling in the menu");

    // And the next run draws again.
    game.start_run(1);
    assert_ne!(game.audio_roll, at_menu, "the next run should redraw it");
}

#[test]
fn dying_redraws_the_soundtrack_on_the_way_back_to_the_menu() {
    let mut game = new_game();
    game.start_run(1);
    // The arrival at the title is spotted by comparing against the previous
    // tick, so the run has to actually tick before it can be left.
    game.tick(&idle());
    let in_run = game.audio_roll;

    game.players[0].hp = -1.0;
    game.players[0].dead = true;
    game.state = State::Title;
    game.tick(&idle());
    assert_ne!(game.audio_roll, in_run, "the menu after a death gets a new one");
}

/* ---------------- melee reach ---------------- */

/// Width of a swinging player's melee box under the given conditions.
fn gun_width(wave: i64, combo: u32, reach: f32) -> f32 {
    let mut game = new_game();
    game.start_run(1);
    game.wave = wave;
    game.players[0].combo = combo;
    game.players[0].attack_ticks = ATTACK_TICKS;
    let v = game.viewport;
    game.players[0].update_gun(&v, reach, wave);
    game.players[0].gun.w
}

#[test]
fn the_attack_box_grows_linearly_with_the_wave() {
    // Equal steps in the wave number are equal steps in reach. That is what
    // separates this from the score-driven version it replaced, which was
    // logarithmic in practice and had finished growing by wave 3.
    let step_early = gun_width(5, 2, 1.0) - gun_width(1, 2, 1.0);
    let step_late = gun_width(9, 2, 1.0) - gun_width(5, 2, 1.0);
    assert!(
        (step_early - step_late).abs() < 0.5,
        "the ramp is not linear: {step_early} then {step_late}"
    );
}

#[test]
fn the_attack_box_stops_growing_at_the_ramp_wave() {
    let at_ten = gun_width(GUN_RAMP_WAVES, 2, 1.0);
    assert!(
        at_ten > gun_width(GUN_RAMP_WAVES - 1, 2, 1.0),
        "it should still be growing one wave short of the end"
    );
    for wave in [GUN_RAMP_WAVES + 1, 20, 40] {
        assert_eq!(gun_width(wave, 2, 1.0), at_ten, "wave {wave} kept growing");
    }
}

#[test]
fn a_full_combo_at_the_ramp_wave_lands_exactly_on_the_ceiling() {
    // The two constants are chosen to meet: the ramp ends where the clamp
    // begins, so neither is doing work the other has already done.
    let v = Viewport::new(1280.0, 800.0);
    assert_eq!(gun_width(GUN_RAMP_WAVES, 2, 1.0), v.wper(GUN_MAX_REACH_PCT));
}

#[test]
fn nothing_pushes_the_attack_box_past_the_ceiling() {
    // A grounded wave doubles reach and a combo doubles it again; together on
    // a late wave they would otherwise span most of the field.
    let v = Viewport::new(1280.0, 800.0);
    let ceiling = v.wper(GUN_MAX_REACH_PCT);
    for wave in [1, 5, 10, 40] {
        for combo in [0, 1, 2] {
            for reach in [1.0, 2.0] {
                let w = gun_width(wave, combo, reach);
                assert!(w <= ceiling, "wave {wave} combo {combo} reach {reach} gave {w}");
            }
        }
    }
}

/* ---------------- how many at once ---------------- */

#[test]
fn the_crowd_is_flat_while_the_roster_is_still_unlocking() {
    // Waves 1-10 change what turns up, not how much of it; the last variant
    // arrives on wave 9.
    for wave in 1..CROWD_RAMP_FIRST_WAVE {
        assert_eq!(
            max_concurrent_enemies(wave),
            MAX_CONCURRENT_ENEMIES,
            "wave {wave} should still be at the opening figure"
        );
    }
}

#[test]
fn the_crowd_grows_by_one_every_two_waves() {
    assert_eq!(max_concurrent_enemies(CROWD_RAMP_FIRST_WAVE), MAX_CONCURRENT_ENEMIES + 1);
    assert_eq!(max_concurrent_enemies(CROWD_RAMP_FIRST_WAVE + 1), MAX_CONCURRENT_ENEMIES + 1);
    assert_eq!(max_concurrent_enemies(CROWD_RAMP_FIRST_WAVE + 2), MAX_CONCURRENT_ENEMIES + 2);
    assert_eq!(max_concurrent_enemies(CROWD_RAMP_FIRST_WAVE + 3), MAX_CONCURRENT_ENEMIES + 2);

    // Never falls, and never gains two at once.
    let mut previous = max_concurrent_enemies(1);
    for wave in 2..80 {
        let now = max_concurrent_enemies(wave);
        assert!(now >= previous, "the crowd shrank at wave {wave}");
        assert!(now - previous <= 1, "the crowd jumped by more than one at wave {wave}");
        previous = now;
    }
}

#[test]
fn the_crowd_stops_at_twice_the_opening_figure() {
    let ceiling = MAX_CONCURRENT_ENEMIES * CROWD_MAX_MULTIPLE;
    for wave in [29, 30, 60, 500] {
        assert_eq!(max_concurrent_enemies(wave), ceiling, "wave {wave} went past the ceiling");
    }
}

#[test]
fn the_wave_manager_honours_the_growing_crowd() {
    // The figure is no use if the spawner keeps its own copy of the old one.
    let mut game = new_game();
    let held = MAX_CONCURRENT_ENEMIES;

    let spawned_at = |wave: i64, live: usize, game: &mut Game| {
        let mut manager = WaveManager::default();
        manager.skip_countdown();
        manager.kind = WaveKind::GroundOnly;
        let mut seen = 0;
        for _ in 0..4000 {
            manager.kind = WaveKind::GroundOnly;
            if let WaveAction::SpawnGround(_) = manager.update(wave, 0, live, &mut game.rng) {
                seen += 1;
            }
        }
        seen
    };

    assert_eq!(spawned_at(1, held, &mut game), 0, "wave 1 should be full at the opening figure");
    assert!(
        spawned_at(CROWD_RAMP_FIRST_WAVE, held, &mut game) > 0,
        "wave {CROWD_RAMP_FIRST_WAVE} should have room for one more"
    );
    assert_eq!(
        spawned_at(CROWD_RAMP_FIRST_WAVE, held + 1, &mut game),
        0,
        "and no room beyond that"
    );
}

/* ---------------- rolled enemies ---------------- */

#[test]
fn a_rolled_enemy_carries_the_health_it_was_promised() {
    // Twice what an armoured one takes: 510 against a quarter armour is 32
    // connecting ticks, about eleven swings.
    let mut game = new_game();
    let v = game.viewport;
    let r = Recipe {
        movement: MoveKind::Run,
        size: Size::Normal,
        shoot: false,
        blink: false,
        shed: false,
        brood: false,
    };
    let z = r.build(&v, 6, 0, &mut game.rng);
    assert_eq!(z.hpmax, ELITE_HP);
    assert_eq!(z.armor, ARMORED_ARMOR);
    let ticks = (z.hpmax / (64.0 * z.armor)).ceil();
    assert_eq!(ticks, 32.0, "it should take 32 connecting ticks");
    assert!(z.elite, "and be marked, having no signature colour of its own");
    assert!(!z.is_boss, "but not be a boss: that flag decides other things");
}

#[test]
fn size_is_independent_of_how_it_moves() {
    // The whole point of the axes: a flier can be the size of a boss and a
    // runner the size of a flyer.
    let mut game = new_game();
    let v = game.viewport;
    let build = |size: Size, movement: MoveKind, g: &mut Game| {
        Recipe { movement, size, shoot: false, blink: false, shed: false, brood: false }
            .build(&v, 6, 0, &mut g.rng)
    };

    let huge_flier = build(Size::Large, MoveKind::Fly, &mut game);
    let slight_runner = build(Size::Flyer, MoveKind::Run, &mut game);

    assert!(matches!(huge_flier.movement, Movement::Fly { .. }));
    assert!(huge_flier.body.w > v.wper(5.0), "large should be larger than usual");
    assert_eq!(slight_runner.movement, Movement::Run);
    assert!(slight_runner.body.h < v.hper(10.0), "a flyer footprint is shallow");
}

#[test]
fn every_size_stands_on_the_floor() {
    // A short one must not hover and a tall one must not sink, or the ground
    // stops meaning anything.
    let mut game = new_game();
    let v = game.viewport;
    for size in Size::ALL {
        let z = Recipe { movement: MoveKind::Run, size, shoot: false, blink: false, shed: false, brood: false }
            .build(&v, 6, 0, &mut game.rng);
        let feet = z.body.y + z.body.h;
        assert!(
            (feet - v.hper(GROUND_Y_PCT)).abs() < 0.01,
            "{size:?} stands with its feet at {feet}"
        );
    }
}

#[test]
fn behaviours_land_where_the_recipe_asked() {
    let mut game = new_game();
    let v = game.viewport;
    let all = Recipe {
        movement: MoveKind::Hop,
        size: Size::Normal,
        shoot: true,
        blink: true,
        shed: true,
        brood: false,
    };
    let z = all.build(&v, 6, 0, &mut game.rng);
    assert!(z.behaviors.shoot.is_some());
    assert!(z.behaviors.blink);
    assert!(z.behaviors.shed);
    assert!(z.max_husks > 0, "a shedder needs somewhere to put them");
    assert!(matches!(z.movement, Movement::Hop { .. }));

    let none = Recipe { shoot: false, blink: false, shed: false, ..all };
    let z = none.build(&v, 6, 0, &mut game.rng);
    assert_eq!(z.behaviors, Behaviors::plain());
    assert_eq!(z.max_husks, 0);
}

#[test]
fn rolling_reaches_every_corner_of_the_space() {
    // If some combination can never come up, the axis it sits on is decoration.
    // Late enough that every trait has been introduced; what happens before
    // then is a separate test.
    let mut rng = Rng::new(20260819);
    let wave = 40;
    let (mut moves, mut sizes) = ([false; 4], [false; 4]);
    let (mut shot, mut blinked, mut shed, mut plain) = (false, false, false, false);
    for _ in 0..4000 {
        let r = Recipe::roll(wave, &mut rng);
        moves[MoveKind::ALL.iter().position(|m| *m == r.movement).unwrap()] = true;
        sizes[Size::ALL.iter().position(|s| *s == r.size).unwrap()] = true;
        shot |= r.shoot;
        blinked |= r.blink;
        shed |= r.shed;
        plain |= !r.shoot && !r.blink && !r.shed;
    }
    assert!(moves.iter().all(|m| *m), "some way of moving never came up");
    assert!(sizes.iter().all(|s| *s), "some size never came up");
    assert!(shot && blinked && shed, "some behaviour never came up");
    assert!(plain, "a plain one - just heavy - should be an ordinary outcome");
}

#[test]
fn a_rolled_enemy_only_carries_what_the_run_has_already_met() {
    // The example this exists for: a wave-six heavy that shoots, breeds and
    // lays hazards, when the shooter, the splitter and the shedder have not
    // appeared once between them.
    let mut rng = Rng::new(6161);
    for _ in 0..4000 {
        let r = Recipe::roll(ELITE_FIRST_WAVE, &mut rng);
        assert!(!r.shoot, "a shooter has not been seen yet");
        assert!(!r.blink, "nor a blinker");
        assert!(!r.shed, "nor a shedder");
        assert!(!r.brood, "nor anything that makes more of itself");
    }
}

#[test]
fn each_trait_waits_for_the_enemy_it_belongs_to() {
    // A rolled enemy is read against the roster - the player knows a hop or a
    // blink because they have met the thing that does it. So each trait waits
    // for the wave its owner arrives on, and is a live possibility from then.
    let cases: [(&str, i64, fn(&Recipe) -> bool); 6] = [
        ("hop", 3, |r| r.movement == MoveKind::Hop),
        ("leap", 4, |r| r.movement == MoveKind::Leap),
        ("boss size", 5, |r| r.size == Size::Large),
        ("brood", 7, |r| r.brood),
        ("blink", 8, |r| r.blink),
        ("shoot", 9, |r| r.shoot),
    ];
    for (name, at, held) in cases {
        let mut rng = Rng::new(at as u64 * 104_729 + 17);
        for _ in 0..4000 {
            assert!(
                !held(&Recipe::roll(at - 1, &mut rng)),
                "{name} came up a wave before the enemy that introduces it"
            );
        }
        let mut rng = Rng::new(at as u64 * 104_729 + 17);
        let seen = (0..4000).any(|_| held(&Recipe::roll(at, &mut rng)));
        assert!(seen, "{name} never came up on the wave it unlocks");
    }
}

#[test]
fn a_shed_trait_waits_for_the_shedder_itself() {
    // Its owner is the one enemy whose wave lives in the config rather than in
    // the spawn table, so it gets its own check against that number.
    let mut rng = Rng::new(777);
    for _ in 0..4000 {
        assert!(
            !Recipe::roll(SHEDDER_MIN_WAVE - 1, &mut rng).shed,
            "a hazard was laid before the shedder ever appeared"
        );
    }
    let mut rng = Rng::new(777);
    assert!(
        (0..4000).any(|_| Recipe::roll(SHEDDER_MIN_WAVE, &mut rng).shed),
        "and never after it did"
    );
}

#[test]
fn the_first_heavies_are_still_worth_meeting() {
    // Holding traits back must not leave the earliest rolled enemies all the
    // same thing: movement and size are open from the start, and between them
    // that is what a wave-six heavy is made of.
    let mut rng = Rng::new(31415);
    let mut moves = [false; 4];
    let mut sizes = [false; 4];
    for _ in 0..4000 {
        let r = Recipe::roll(ELITE_FIRST_WAVE, &mut rng);
        moves[MoveKind::ALL.iter().position(|m| *m == r.movement).unwrap()] = true;
        sizes[Size::ALL.iter().position(|s| *s == r.size).unwrap()] = true;
    }
    assert!(moves.iter().all(|m| *m), "some way of moving is missing at wave six");
    assert!(sizes.iter().all(|s| *s), "some size is missing at wave six");
}

#[test]
fn young_are_held_to_the_same_schedule_as_their_parent() {
    // A brood must not be a way round the timetable: the parent needed wave
    // seven to breed at all, and what it breeds is drawn against that wave too.
    let mut game = new_game();
    let v = game.viewport;
    game.state = State::Playing;
    game.wave = unlock::BROOD;
    let recipe = Recipe {
        movement: MoveKind::Run,
        size: Size::Normal,
        shoot: false,
        blink: false,
        shed: false,
        brood: true,
    };
    game.zombies.clear();
    game.flyers.clear();
    game.zombies.push(recipe.build(&v, game.wave, 0, &mut game.rng));

    for _ in 0..12 {
        let target = game.zombies[0].body;
        game.players[0].attack_ticks = 0;
        game.zombies[0].body = target;
        game.zombies[0].hp = game.zombies[0].hpmax;
        swing_at(&mut game, 0, target);
        game.tick(&idle());
    }
    let young: Vec<_> = game
        .zombies
        .iter()
        .filter(|z| z.color == ELITE_COLOR && !z.elite)
        .collect();
    assert!(!young.is_empty(), "nothing was bred, so nothing is proven");
    for z in young {
        assert!(z.behaviors.shoot.is_none(), "a child shot before the shooter existed");
        assert!(!z.behaviors.blink, "a child blinked before the blinker existed");
        assert!(!z.behaviors.shed, "a child shed before the shedder existed");
    }
}

#[test]
fn a_rolled_enemy_pays_for_the_work_it_took() {
    // Eleven swings for the ordinary six points would be an insult, and the
    // boss flag cannot be borrowed for it: that flag also decides how the wall
    // treats it.
    let mut game = new_game();
    let v = game.viewport;
    let plain = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    let elite = Recipe { movement: MoveKind::Run, size: Size::Normal, shoot: false, blink: false, shed: false, brood: false }
        .build(&v, 10, 0, &mut game.rng);
    assert!(elite.reward > plain.reward * 10, "it should pay like the work it is");
}

/* ---------------- when rolled heavies arrive ---------------- */

/// Runs a wave's worth of spawn decisions and reports how many heavies it
/// asked for.
fn elites_in_wave(wave: i64) -> usize {
    let mut manager = WaveManager::default();
    let mut rng = Rng::new(wave as u64 * 7919 + 1);
    manager.begin_wave(wave, &mut rng);
    manager.skip_countdown();
    let mut count = 0;
    let mut spawned = 0i64;
    // Room is reported as empty throughout, so the budget is the only limit.
    for _ in 0..40_000 {
        match manager.update(wave, spawned, 0, &mut rng) {
            WaveAction::Idle => {}
            WaveAction::ClearWave => break,
            WaveAction::SpawnElite(n) => {
                count += n;
                spawned += n as i64;
            }
            _ => spawned += 1,
        }
    }
    count
}

#[test]
fn rolled_heavies_start_at_the_sixth_wave() {
    // Late enough that the fixed roster has been met: a rolled enemy only reads
    // as unusual against a set the player already knows.
    for wave in 1..ELITE_FIRST_WAVE {
        assert_eq!(elites_in_wave(wave), 0, "wave {wave} should be off the list");
    }
    assert_eq!(elites_in_wave(ELITE_FIRST_WAVE), 1);
}

#[test]
fn one_rolled_heavy_a_wave_until_the_ramp_and_never_on_a_boss_wave() {
    for wave in ELITE_FIRST_WAVE..ELITE_RAMP_FIRST_WAVE {
        let want = if wave % 5 == 0 { 0 } else { 1 };
        assert_eq!(
            elites_in_wave(wave),
            want,
            "wave {wave} produced the wrong number of heavies"
        );
    }
}

/// Runs a wave and reports every arrival of heavies: how many came at once,
/// and at what fraction of the wave's budget.
fn elite_arrivals(wave: i64) -> alloc::vec::Vec<(usize, i64)> {
    let mut manager = WaveManager::default();
    let mut rng = Rng::new(wave as u64 * 31 + 7);
    manager.begin_wave(wave, &mut rng);
    manager.skip_countdown();
    let budget = crate::config::wave_budget(wave).max(1);
    let mut out = alloc::vec::Vec::new();
    let mut spawned = 0i64;
    for _ in 0..200_000 {
        match manager.update(wave, spawned, 0, &mut rng) {
            WaveAction::Idle => {}
            WaveAction::ClearWave => break,
            WaveAction::SpawnElite(n) => {
                out.push((n, spawned * 100 / budget));
                spawned += n as i64;
            }
            _ => spawned += 1,
        }
    }
    out
}

#[test]
fn the_eighteenth_wave_is_seven_heavies_evenly_spread() {
    // Straight from the specification, and the reason the spacing divides by
    // one more than the count: seven arrivals want seven gaps between them and
    // a gap at each end, or the last one lands exactly as the wave runs out.
    let arrivals = elite_arrivals(18);
    assert_eq!(arrivals.len(), 7, "seven separate arrivals");
    assert_eq!(arrivals.iter().map(|(n, _)| n).sum::<usize>(), 7);
    assert!(arrivals.iter().all(|(n, _)| *n == 1), "one at a time this early");

    let at: alloc::vec::Vec<i64> = arrivals.iter().map(|(_, p)| *p).collect();
    assert!(at[0] > 5, "the wave should open as an ordinary one, not at {}%", at[0]);
    assert!(*at.last().unwrap() < 95, "and not finish on one either");
    // Evenly: every gap the same, to within the rounding of whole spawns.
    let gaps: alloc::vec::Vec<i64> = at.windows(2).map(|w| w[1] - w[0]).collect();
    let (lo, hi) = (
        *gaps.iter().min().unwrap(),
        *gaps.iter().max().unwrap(),
    );
    assert!(hi - lo <= 2, "the gaps are uneven: {gaps:?}");
}

#[test]
fn the_twenty_second_wave_is_thirteen_in_pairs_and_one_alone() {
    // The other worked example: thirteen does not halve, so six pairs and a
    // single - seven arrivals, the same number of arrivals as the eighteenth
    // wave had. That is what pairing them up buys.
    let arrivals = elite_arrivals(22);
    assert_eq!(arrivals.iter().map(|(n, _)| n).sum::<usize>(), 13);
    assert_eq!(arrivals.len(), 7, "seven arrivals");
    assert_eq!(arrivals.iter().filter(|(n, _)| *n == 2).count(), 6, "six pairs");
    assert_eq!(arrivals.iter().filter(|(n, _)| *n == 1).count(), 1, "and one alone");
}

#[test]
fn the_ramp_climbs_by_two_then_by_one_and_then_stops() {
    let counts: alloc::vec::Vec<usize> = (15..=27).map(elite_count).collect();
    assert_eq!(
        counts,
        alloc::vec![1, 3, 5, 7, 9, 11, 12, 13, 14, 15, 16, 16, 16],
        "the ramp does not match the one that was asked for"
    );
}

#[test]
fn a_wave_stops_getting_bigger_where_it_starts_getting_heavier() {
    // The two halves of the same decision: past the ramp a wave is not larger
    // than the one before it, it is made of worse things. Growing *and* turning
    // heavy would be two escalations stacked on each other.
    for wave in 1..ELITE_RAMP_FIRST_WAVE {
        assert_eq!(wave_budget(wave), wave * 10);
    }
    let frozen = wave_budget(ELITE_RAMP_FIRST_WAVE);
    for wave in ELITE_RAMP_FIRST_WAVE..40 {
        assert_eq!(wave_budget(wave), frozen, "wave {wave} grew");
    }
}

#[test]
fn heavies_replace_ordinary_enemies_rather_than_joining_them() {
    // Each one spends a slot out of the wave's budget, so a wave of a hundred
    // and sixty with sixteen heavies is a hundred and forty-four ordinary ones
    // and not a hundred and sixty.
    let wave = 22;
    let mut manager = WaveManager::default();
    let mut rng = Rng::new(4242);
    manager.begin_wave(wave, &mut rng);
    manager.skip_countdown();
    let (mut spawned, mut heavies, mut plain) = (0i64, 0usize, 0usize);
    for _ in 0..200_000 {
        match manager.update(wave, spawned, 0, &mut rng) {
            WaveAction::Idle => {}
            WaveAction::ClearWave => break,
            WaveAction::SpawnElite(n) => {
                heavies += n;
                spawned += n as i64;
            }
            _ => {
                plain += 1;
                spawned += 1;
            }
        }
    }
    assert_eq!(heavies, elite_count(wave));
    assert_eq!(
        (heavies + plain) as i64,
        wave_budget(wave),
        "the wave should still be exactly its budget"
    );
}

#[test]
fn past_the_last_ramping_wave_the_groups_grow_instead() {
    // Nothing left to add, so what escalates is how much lands at once - and it
    // walks there rather than jumping. A wave that went straight from eight
    // arrivals to one would not be harder than the wave before it, it would be
    // a wall.
    let sizes: alloc::vec::Vec<usize> = (25..=32).map(elite_group_size).collect();
    assert_eq!(sizes, alloc::vec![2, 3, 4, 5, 6, 7, 8, 9]);

    // Every wave past the ramp is still the same number of heavies, arriving
    // in fewer and fewer pieces.
    let mut previous = usize::MAX;
    for wave in 26..40i64 {
        if wave % 5 == 0 {
            continue;
        }
        let arrivals = elite_arrivals(wave);
        assert_eq!(
            arrivals.iter().map(|(n, _)| n).sum::<usize>(),
            elite_count(wave),
            "wave {wave} owes the wrong number"
        );
        assert!(
            arrivals.len() <= previous,
            "wave {wave} came in more pieces than the wave before it"
        );
        previous = arrivals.len();
    }

    // And it does arrive all at once in the end, without a step to get there.
    assert_eq!(elite_group_size(60), elite_count(60));
}

#[test]
fn a_heavy_waits_until_the_wave_is_under_way() {
    // The wave should open as an ordinary one and then turn, rather than
    // starting with the hardest thing in it alone on an empty field.
    let wave = 7;
    let mut manager = WaveManager::default();
    let mut rng = Rng::new(4242);
    manager.begin_wave(wave, &mut rng);
    manager.skip_countdown();
    let mut spawned = 0i64;
    for _ in 0..40_000 {
        match manager.update(wave, spawned, 0, &mut rng) {
            WaveAction::Idle => {}
            WaveAction::ClearWave => break,
            WaveAction::SpawnElite(_) => {
                assert!(spawned > 0, "it walked in before anything else did");
                assert!(
                    spawned >= wave * 10 / ELITE_ENTRY_FRACTION,
                    "it walked in after only {spawned} of the wave's budget"
                );
                return;
            }
            _ => spawned += 1,
        }
    }
    panic!("no heavy arrived at all");
}

#[test]
fn a_heavy_reaches_the_field_and_is_named_on_the_way_in() {
    let mut game = new_game();
    game.state = State::Playing;
    game.wave = ELITE_FIRST_WAVE;
    game.spawn_count = ELITE_FIRST_WAVE * 10;
    game.waves.skip_countdown();

    for _ in 0..600 {
        game.tick(&idle());
        if game.zombies.iter().any(|z| z.elite) {
            break;
        }
    }
    let heavy = game
        .zombies
        .iter()
        .find(|z| z.elite)
        .expect("no rolled heavy arrived");
    assert_eq!(heavy.hpmax, ELITE_HP);
    // Constructors place enemies against the view; the spawn site is where a
    // view-relative x becomes a world one. Either edge is fair - what must not
    // happen is it landing a whole field away because the offset was skipped.
    let from_view = heavy.body.x - game.camera_x;
    assert!(
        (-heavy.body.w..=game.viewport.w).contains(&from_view),
        "it entered at {from_view} from the view edge"
    );
    let name = game.elite_notice().expect("it arrived without a name");
    assert!(!name.is_empty(), "the one thing that cannot be read off a colour");
}

#[test]
fn the_name_does_not_stay_up_forever() {
    let mut game = new_game();
    game.elite_notice = Some(("HOPPER SMALL GUN".to_string(), 0));
    game.timer = ELITE_NOTICE_TICKS - 1;
    assert!(game.elite_notice().is_some());
    game.timer = ELITE_NOTICE_TICKS;
    assert!(game.elite_notice().is_none(), "it should have cleared by now");
}

#[test]
fn a_heavy_from_the_last_run_is_not_announced_in_the_next_one() {
    // The clock goes back to zero at the start of a run. An age read as
    // `timer - at` then comes out negative, which is "recent" to any test
    // written as a single bound.
    let mut game = new_game();
    game.timer = 5_000;
    game.elite_notice = Some(("HUGE FLIER GUN".to_string(), 4_950));
    assert!(game.elite_notice().is_some());
    game.start_new_run(1);
    assert!(
        game.elite_notice().is_none(),
        "the last run's heavy was announced in this one"
    );
}

#[test]
fn what_a_heavy_turns_out_to_be_is_not_shifted_by_the_fight() {
    // Same reason the offer has its own generator: two runs that reach wave 6
    // the same way should meet the same enemy, whatever happened in between.
    let names = |swings: usize| {
        let mut game = new_game();
        game.state = State::Playing;
        game.wave = ELITE_FIRST_WAVE;
        game.spawn_count = ELITE_FIRST_WAVE * 10;
        game.waves.skip_countdown();
        for i in 0..600 {
            // The main generator gets churned by the fight; the elite one
            // must not notice.
            if i < swings {
                game.players[0].strike_id = game.players[0].strike_id.wrapping_add(1).max(1);
                let _ = game.rng.unit();
            }
            game.tick(&idle());
            if let Some(name) = game.elite_notice() {
                return name.to_string();
            }
        }
        String::new()
    };
    let quiet = names(0);
    assert!(!quiet.is_empty(), "nothing arrived, so nothing is proven");
    assert_eq!(quiet, names(120), "the fight changed which enemy showed up");
}

#[test]
fn a_rolled_heavy_brings_the_boss_track_with_it() {
    let mut game = new_game();
    game.start_run(1);
    game.state = State::Playing;
    game.waves.skip_countdown();
    game.zombies.clear();
    game.flyers.clear();
    assert!(!game.music_state().boss, "nothing on the field yet");

    let v = game.viewport;
    let plain = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    game.zombies.push(plain);
    assert!(!game.music_state().boss, "an ordinary enemy is not an occasion");

    let heavy = Recipe {
        movement: MoveKind::Run,
        size: Size::Normal,
        shoot: false,
        blink: false,
        shed: false,
        brood: false,
    }
    .build(&v, ELITE_FIRST_WAVE, 0, &mut game.rng);
    game.zombies.push(heavy);
    assert!(game.music_state().boss, "a rolled heavy should bring the track");

    game.zombies.retain(|z| !z.elite);
    assert!(!game.music_state().boss, "and take it away again when it dies");
}

#[test]
fn young_do_not_keep_the_boss_track_playing() {
    // They wear the parent's colour but they are ordinary enemies, and a field
    // of them after the parent is dead is not still a boss fight.
    let mut game = new_game();
    game.start_run(1);
    game.state = State::Playing;
    game.waves.skip_countdown();
    game.zombies.clear();
    game.flyers.clear();

    let v = game.viewport;
    let minion = Recipe {
        movement: MoveKind::Run,
        size: Size::Normal,
        shoot: false,
        blink: false,
        shed: false,
        brood: false,
    }
    .build_minion(&v, 0, &mut game.rng);
    game.zombies.push(minion);
    assert!(!game.music_state().boss);
}

/* ---------------- the brood ---------------- */

/// Young, told apart from what the wave is spawning alongside them.
///
/// Counting the whole list would be counting the wave manager's work as well:
/// it keeps feeding the field while this runs, and an ordinary zombie looks
/// just like a minion in health and in every flag. The colour does not lie -
/// only a rolled enemy and its young wear it.
fn minions(game: &Game) -> usize {
    game.zombies
        .iter()
        .filter(|z| z.color == ELITE_COLOR && !z.elite)
        .count()
}

/// A brooder standing in front of player 0, in a state where a swing will land.
fn brooder(game: &mut Game) -> usize {
    let v = game.viewport;
    let recipe = Recipe {
        movement: MoveKind::Run,
        size: Size::Normal,
        shoot: false,
        blink: false,
        shed: false,
        brood: true,
    };
    let z = recipe.build(&v, 6, 0, &mut game.rng);
    game.zombies.clear();
    game.flyers.clear();
    game.zombies.push(z);
    game.state = State::Playing;
    0
}

#[test]
fn a_brooder_answers_a_swing_with_young() {
    let mut game = new_game();
    brooder(&mut game);
    let target = game.zombies[0].body;
    swing_at(&mut game, 0, target);
    game.tick(&idle());
    assert!(minions(&game) > 0, "the blow should have produced young");
    for z in game.zombies.iter().filter(|z| !z.elite) {
        assert!(!z.broods, "a brood that broods never stops");
    }
    for z in game.zombies.iter().filter(|z| z.color == ELITE_COLOR && !z.elite) {
        assert_eq!(z.hpmax, 255.0, "they die to one swing like anything else");
    }
}

#[test]
fn one_brood_per_swing_and_not_per_tick() {
    // The distinction the whole feature turns on: a swing covers an enemy for
    // most of a dozen ticks. Paying per tick would fill the field from one hit.
    let mut game = new_game();
    brooder(&mut game);
    let target = game.zombies[0].body;
    swing_at(&mut game, 0, target);

    let mut counts = alloc::vec![minions(&game)];
    for _ in 0..ATTACK_TICKS {
        // Pin it in place so the swing keeps reaching it for the whole arc.
        game.zombies[0].body = target;
        game.tick(&idle());
        counts.push(minions(&game));
    }
    let grew = counts.windows(2).filter(|w| w[1] > w[0]).count();
    assert_eq!(grew, 1, "it should have bred exactly once, not {grew} times");
}

#[test]
fn a_second_swing_breeds_again() {
    let mut game = new_game();
    brooder(&mut game);
    let target = game.zombies[0].body;

    swing_at(&mut game, 0, target);
    game.tick(&idle());
    let after_first = minions(&game);

    game.players[0].attack_ticks = 0;
    game.zombies[0].body = target;
    swing_at(&mut game, 0, target);
    game.tick(&idle());
    assert!(
        minions(&game) > after_first,
        "a fresh swing is a fresh blow, so it should breed again"
    );
}

#[test]
fn a_brood_cannot_outgrow_the_wave_it_is_in() {
    // Otherwise a brooder is not a hard enemy, it is a losing race: every blow
    // that fails to kill it leaves more on the field than before.
    let mut game = new_game();
    brooder(&mut game);
    let cap = max_concurrent_enemies(game.wave);
    let ceiling = cap + BROOD_MAX as usize;
    let target = game.zombies[0].body;

    for _ in 0..40 {
        game.players[0].attack_ticks = 0;
        game.zombies[0].body = target;
        game.zombies[0].hp = game.zombies[0].hpmax;
        swing_at(&mut game, 0, target);
        game.tick(&idle());
        assert!(
            game.zombies.len() + game.flyers.len() <= ceiling,
            "the field ran past the crowd limit"
        );
    }
}

#[test]
fn the_wall_does_not_seed_the_field() {
    // The ultimate exists to clear a field, not to fill one.
    let mut game = new_game();
    brooder(&mut game);
    game.zombies[0].hp = ELITE_HP; // survives the wall's 300
    let target = game.zombies[0].body;
    raise_wall_over(&mut game, target);
    for _ in 0..20 {
        game.zombies[0].body = target;
        game.tick(&idle());
    }
    assert_eq!(minions(&game), 0, "the wall bred it");
    assert!(game.zombies[0].hp < ELITE_HP, "but the wall should still bite");
}

/* ---------------- movement and behaviour axes ---------------- */

#[test]
fn the_named_variants_land_on_the_axis_they_belong_to() {
    // Moving is a choice and behaving is a set. A preset that put its trick on
    // the wrong axis would quietly become uncombinable, or exclusive with
    // something it has no quarrel with.
    let mut game = new_game();
    let v = game.viewport;

    let jumper = Zombie::jumper(&v, &mut game.rng);
    assert!(matches!(jumper.movement, Movement::Hop { .. }));
    assert_eq!(jumper.behaviors, Behaviors::plain());

    let leaper = Zombie::leaper(&v, &mut game.rng);
    assert!(matches!(leaper.movement, Movement::Leap(_)));

    let shooter = Zombie::shooter(&v, &mut game.rng);
    assert_eq!(shooter.movement, Movement::Run, "shooting is not a way of moving");
    assert!(shooter.behaviors.shoot.is_some());

    let shedder = Zombie::shedder(&v, &mut game.rng);
    assert_eq!(shedder.movement, Movement::Run);
    assert!(shedder.behaviors.shed);
}

#[test]
fn behaviours_combine_where_movements_cannot() {
    // The whole point of the split: one enemy may shoot and shed and blink at
    // once, and the type still only lets it move one way.
    let mut game = new_game();
    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.behaviors.shoot = Some(0);
    z.behaviors.shed = true;
    z.behaviors.blink = true;
    z.movement = Movement::Hop { cooldown: 0 };

    assert!(z.behaviors.shoot.is_some() && z.behaviors.shed && z.behaviors.blink);
    assert!(matches!(z.movement, Movement::Hop { .. }));
}

#[test]
fn a_flying_enemy_rides_an_arc_instead_of_falling() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    clear_arena(&mut game);

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.movement = Movement::Fly { offset: 0 };
    z.hp = 1_000_000.0;
    z.hpmax = z.hp;
    game.zombies.push(z);

    // y grows downward, so the top of the arc is the smallest number.
    let ground = v.hper(GROUND_Y_PCT);
    let (mut top, mut bottom) = (f32::MAX, f32::MIN);
    for _ in 0..400 {
        game.flyers.clear();
        game.tick(&idle());
        let Some(z) = game.zombies.iter().find(|z| z.hpmax > 1000.0) else {
            panic!("the flier died");
        };
        top = top.min(z.body.y);
        bottom = bottom.max(z.body.y);
    }

    // It climbs to the ceiling of the arc, which gravity would never allow.
    assert!(
        top <= v.hper(FLYER_ARC_TOP_PCT) + v.hper(2.0),
        "it only climbed to {top}"
    );
    // And it dips to where a player standing on the floor can still reach it,
    // for the same reason an ordinary flyer's arc bottoms out where it does -
    // without ever resting on the floor itself.
    assert!(
        bottom >= v.hper(FLYER_ARC_BOTTOM_PCT) - v.hper(2.0),
        "the dip only reached {bottom}"
    );
    assert!(bottom < ground, "it should never come to rest on the floor");
}

#[test]
fn a_flying_enemy_still_chases() {
    // Flying replaces gravity, not the walk: it should close on the player the
    // way anything else does, or it would just bob in place.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    clear_arena(&mut game);

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.movement = Movement::Fly { offset: 0 };
    z.hp = 1_000_000.0;
    z.hpmax = z.hp;
    z.body.x = game.players[0].body.x + v.wper(60.0);
    let started = z.body.x;
    game.zombies.push(z);

    for _ in 0..60 {
        game.flyers.clear();
        game.tick(&idle());
    }
    let now = game.zombies.iter().find(|z| z.hpmax > 1000.0).unwrap().body.x;
    assert!(now < started, "it drifted away instead of closing in");
}

/* ---------------- the wall against bosses ---------------- */

/// Puts an active ultimate field over `body`, as if the player had slammed.
fn raise_wall_over(game: &mut Game, body: Body) {
    game.players[0].field.active = true;
    game.players[0].field.body = body;
    // Raising a wall for real takes a fresh action number, which is what stops
    // it landing on the same enemy on every one of the ~17 ticks it stands.
    // A hand-made one has to take it too.
    game.players[0].strike_id = game.players[0].strike_id.wrapping_add(1).max(1);
}

#[test]
fn the_wall_takes_a_fixed_share_off_a_ground_boss() {
    let mut game = new_game();
    game.start_run(1);
    game.wave = 5;
    clear_arena(&mut game);

    let mut boss = Zombie::boss(&game.viewport, game.wave, &mut game.rng);
    boss.body.x = game.players[0].body.x;
    boss.body.y = game.players[0].body.y;
    let (full, body) = (boss.hpmax, boss.body);
    game.zombies.push(boss);

    raise_wall_over(&mut game, body);
    game.tick(&idle());

    let boss = game
        .zombies
        .iter()
        .find(|z| z.is_boss)
        .expect("the boss died to a single wall");
    let taken = full - boss.hp;
    assert!(
        (taken - full * FIELD_BOSS_FRACTION).abs() < 1.0,
        "a wall took {taken} off {full}, wanted a seventh"
    );
}

#[test]
fn the_wall_no_longer_ends_the_flying_boss_outright() {
    // It used to be an instant kill whatever the health, which meant one
    // charge finished the wave-10 boss while no number of walls could have
    // finished a ground one.
    let mut game = new_game();
    game.start_run(1);
    clear_arena(&mut game);

    let size_ref = game.players[0].body;
    let timer = game.timer;
    let mut boss = Flyer::flying_boss(&game.viewport, &size_ref, timer, &mut game.rng);
    boss.body.x = game.players[0].body.x;
    boss.body.y = game.players[0].body.y;
    let (full, body) = (boss.hpmax, boss.body);
    game.flyers.push(boss);

    raise_wall_over(&mut game, body);
    game.tick(&idle());

    let boss = game
        .flyers
        .iter()
        .find(|f| f.is_boss)
        .expect("one wall still ended it");
    let taken = full - boss.hp;
    assert!(
        (taken - full * FIELD_BOSS_FRACTION).abs() < 1.0,
        "a wall took {taken} off {full}, wanted a seventh"
    );
}

#[test]
fn the_wall_still_ends_anything_ordinary() {
    let mut game = new_game();
    game.start_run(1);
    clear_arena(&mut game);

    let mut z = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.x = game.players[0].body.x;
    z.body.y = game.players[0].body.y;
    let body = z.body;
    game.zombies.push(z);

    raise_wall_over(&mut game, body);
    // Two ticks: the death check runs at the top of the pass that applied the
    // damage, so a kill is collected on the next one.
    game.tick(&idle());
    game.tick(&idle());
    assert!(
        !game.zombies.iter().any(|z| z.color == TEST_ENEMY_COLOR),
        "the wall should still clear ordinary enemies outright"
    );
}

#[test]
fn the_wall_wears_down_something_heavier_instead_of_deleting_it() {
    // The point of the flat number: an enemy built to take punishment should
    // take it from the wall too. It used to set health to -1 whatever the enemy
    // was, so no heavy one could exist below boss rank.
    let mut game = new_game();
    game.start_run(1);
    clear_arena(&mut game);

    let mut z = Zombie::from_edge(&game.viewport, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.x = game.players[0].body.x;
    z.body.y = game.players[0].body.y;
    // Twice an armoured one, which is what a mini-boss is meant to carry.
    z.hp = 255.0 * 2.0;
    z.hpmax = z.hp;
    let body = z.body;
    game.zombies.push(z);

    raise_wall_over(&mut game, body);
    game.tick(&idle());
    game.tick(&idle());
    let survivor = game
        .zombies
        .iter()
        .find(|z| z.color == TEST_ENEMY_COLOR)
        .expect("510 health should not fall to one wall");
    assert_eq!(survivor.hp, 510.0 - FIELD_DAMAGE);

    // And the second wall finishes it, so it is worn down rather than immune.
    raise_wall_over(&mut game, body);
    game.tick(&idle());
    game.tick(&idle());
    assert!(
        !game.zombies.iter().any(|z| z.color == TEST_ENEMY_COLOR),
        "a second wall should finish it"
    );
}

#[test]
fn one_wall_still_clears_every_ordinary_variant() {
    // The flat number is just past the 255 they all carry, so nothing that
    // existed before the change survives a touch. This is the guard on that.
    let build: [(&str, fn(&Viewport, &mut Rng) -> Zombie); 8] = [
        ("runt", Zombie::runt),
        ("jumper", Zombie::jumper),
        ("leaper", Zombie::leaper),
        ("armored", Zombie::armored),
        ("frenzied", Zombie::frenzied),
        ("splitter", Zombie::splitter),
        ("blinker", Zombie::blinker),
        ("shooter", Zombie::shooter),
    ];
    for (name, make) in build {
        let mut game = new_game();
        game.start_run(1);
        clear_arena(&mut game);

        let v = game.viewport;
        let mut z = make(&v, &mut game.rng);
        z.body.x = game.players[0].body.x;
        z.body.y = game.players[0].body.y;
        // By colour, not by health: two ticks is long enough for the wave to
        // spawn another enemy carrying the same 255, and it would read as a
        // survivor.
        let marker = z.color;
        let body = z.body;
        game.zombies.push(z);

        raise_wall_over(&mut game, body);
        game.tick(&idle());
        game.tick(&idle());
        assert!(
            !game.zombies.iter().any(|z| z.color == marker),
            "the wall no longer clears a {name}"
        );
    }
}

/* ---------------- enemy colour ---------------- */

#[test]
fn plain_enemies_wear_a_player_colour() {
    // Colour is the only thing telling variants apart, so a base enemy rolling
    // its own out of the same range could turn up dressed as a jumper.
    let mut game = new_game();
    game.start_run(2);
    let wanted: Vec<_> = game.players.iter().map(|p| p.color).collect();

    game.waves.skip_countdown();
    for _ in 0..900 {
        // Held every tick: the manager re-rolls the kind when a wave turns over.
        game.waves.kind = WaveKind::BasicOnly;
        game.tick(&idle());
        if game.zombies.len() >= 6 {
            break;
        }
    }
    assert!(!game.zombies.is_empty(), "no plain enemies ever spawned");
    for z in game.zombies.iter() {
        assert!(
            wanted.contains(&z.color),
            "a plain enemy wore {:?}, which belongs to no player",
            z.color
        );
    }
}

/* ---------------- attack kinds ---------------- */

/// Damage one swing of `kind` puts into a target that cannot die.
///
/// Measured rather than read off the constants, because what a swing actually
/// lands depends on how long the enemy stays inside the box - and that is
/// decided by the throw, not by the damage number.
fn swing_damage(kind: AttackKind, level: u8) -> f32 {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].attack = kind;
    game.players[0].attack_level = level;

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(6.0);
    z.hp = 1_000_000.0;
    z.hpmax = 1_000_000.0;
    let body = z.body;
    game.zombies.push(z);
    game.flyers.clear();

    let before = game.zombies[0].hp;
    swing_at(&mut game, 0, body);
    for _ in 0..(kind.swing_ticks(ATTACK_TICKS) + 2) {
        game.flyers.clear();
        game.tick(&idle());
    }
    before - game.zombies[0].hp
}

#[test]
fn an_ordinary_swing_is_the_yardstick() {
    // Three connecting ticks of sixty-four: the throw clears the enemy out of
    // the box halfway through the swing. Every kind below is measured against
    // this, so if it moves, all of those trades move with it.
    assert_eq!(swing_damage(AttackKind::Basic, 1), 192.0);
}

#[test]
fn piercing_trades_safety_for_more_of_the_swing() {
    // It never throws, so the enemy is not cleared out of the box a third of
    // the way through - and it stays in touching range for as long as it is in
    // there. Measured at five connecting ticks against an ordinary swing's
    // three, not the six the box is out for: with nothing pushing it, the enemy
    // walks itself through the far edge before the swing ends.
    let basic = swing_damage(AttackKind::Basic, 1);
    let pierce = swing_damage(AttackKind::Piercing, 1);
    assert_eq!(pierce, 64.0 * 5.0, "five connecting ticks");
    assert!(pierce > basic * 1.5, "well past what an ordinary swing manages");
    assert!(swing_damage(AttackKind::Piercing, 3) > pierce, "levels should add");
}

#[test]
fn the_hammer_lands_once_however_long_it_is_out() {
    // Twelve ticks of contact must not mean twelve hits: that would be eight
    // ordinary swings rather than a heavy version of one.
    let hammer = swing_damage(AttackKind::Hammer, 1);
    assert_eq!(hammer, 64.0 * 6.0, "one blow, not a stream of them");
    assert!(
        hammer < swing_damage(AttackKind::Piercing, 1) * 2.0,
        "a single blow should not out-damage a kind that gets every tick"
    );
}

#[test]
fn a_single_hit_does_not_depend_on_the_geometry() {
    // The point of it is a floor under the damage, so the number must not
    // depend on where the enemy stood or where the throw sent it.
    let hit = swing_damage(AttackKind::SingleHit, 1);
    assert_eq!(hit, 64.0 * 4.0);
    assert!(hit > swing_damage(AttackKind::Basic, 1), "and it beats the average");
}

#[test]
fn a_sweep_reaches_behind_as_well_as_in_front() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].attack = AttackKind::Thin;
    game.players[0].facing_right = true;
    game.players[0].attack_ticks = ATTACK_TICKS;
    let v = game.viewport;
    game.players[0].update_gun(&v, 1.0, game.wave);

    let front = game.players[0].gun;
    let back = game.players[0].gun_back.expect("a sweep has a second box");
    assert!(back.x < front.x, "the second box should be on the other side");
    assert_eq!(back.w, front.w, "and the same size");
    assert_eq!(back.h, front.h);
}

#[test]
fn a_sweep_is_worth_swinging_at_its_first_level() {
    // It started at a quarter of an ordinary swing per side and played too
    // short to be worth the height it gives up.
    let side = |level: u8| {
        let mut game = new_game();
        game.start_run(1);
        settle_on_floor(&mut game);
        game.players[0].attack = AttackKind::Thin;
        game.players[0].attack_level = level;
        game.players[0].attack_ticks = ATTACK_TICKS;
        let v = game.viewport;
        game.players[0].update_gun(&v, 1.0, game.wave);
        game.players[0].gun.w
    };
    let basic = {
        let mut game = new_game();
        game.start_run(1);
        settle_on_floor(&mut game);
        game.players[0].attack_ticks = ATTACK_TICKS;
        let v = game.viewport;
        game.players[0].update_gun(&v, 1.0, game.wave);
        game.players[0].gun.w
    };

    let sweep = side(1);
    assert!(
        (sweep - basic * 0.8).abs() < 0.01,
        "a first-level sweep reaches {sweep} against an ordinary {basic}"
    );
    // Both sides together well past one ordinary swing: that is the trade for
    // giving up the height.
    assert!(sweep * 2.0 > basic * 1.5, "two sides should add up to more than one");
    assert!(side(3) > sweep, "levels still lengthen it");
}

#[test]
fn a_sweep_sits_low_and_a_heavy_swing_reaches_down() {
    // The sweep gives up height and keeps its bottom edge, so it still catches
    // what stands on the ground while flyers pass over. The tall kind keeps the
    // top edge and grows down into that same low ground.
    let swing_box = |kind: AttackKind| {
        let mut game = new_game();
        game.start_run(1);
        settle_on_floor(&mut game);
        game.players[0].attack = kind;
        game.players[0].attack_ticks = ATTACK_TICKS;
        let v = game.viewport;
        game.players[0].update_gun(&v, 1.0, game.wave);
        game.players[0].gun
    };

    let basic = swing_box(AttackKind::Basic);
    let thin = swing_box(AttackKind::Thin);
    let tall = swing_box(AttackKind::Tall);

    assert!(thin.h < basic.h, "a sweep is shallower");
    assert!(
        (thin.y + thin.h - (basic.y + basic.h)).abs() < 0.01,
        "and it keeps the bottom edge, not the top"
    );
    assert!(tall.h > basic.h, "a heavy swing is deeper");
    assert!((tall.y - basic.y).abs() < 0.01, "and it keeps the top edge");
    assert!(tall.w < basic.w, "paid for with reach");
}

#[test]
fn a_lunge_starts_short_and_ends_long() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].attack = AttackKind::Lunge;
    let v = game.viewport;

    game.players[0].attack_ticks = ATTACK_TICKS;
    game.players[0].update_gun(&v, 1.0, game.wave);
    let first = game.players[0].gun.w;

    game.players[0].attack_ticks = 1;
    game.players[0].update_gun(&v, 1.0, game.wave);
    let last = game.players[0].gun.w;

    assert!(last > first * 2.0, "a lunge should be reaching by the end");
}

#[test]
fn the_resting_box_is_the_same_whatever_the_upgrade() {
    // It is the direction indicator and the dash readout, not a preview of the
    // swing, so no upgrade may move it.
    let resting = |kind: AttackKind| {
        let mut game = new_game();
        game.start_run(1);
        settle_on_floor(&mut game);
        game.players[0].attack = kind;
        game.players[0].attack_ticks = 0;
        let v = game.viewport;
        game.players[0].update_gun(&v, 1.0, game.wave);
        (game.players[0].gun, game.players[0].gun_back)
    };

    let (basic, basic_back) = resting(AttackKind::Basic);
    assert!(basic_back.is_none());
    for kind in AttackKind::ALL {
        let (b, back) = resting(kind);
        assert_eq!(b.w, basic.w, "{kind:?} moved the resting box");
        assert_eq!(b.h, basic.h, "{kind:?} moved the resting box");
        assert_eq!(b.y, basic.y, "{kind:?} moved the resting box");
        assert!(back.is_none(), "{kind:?} left a second box at rest");
    }
}

/* ---------------- thrown and placed attacks ---------------- */

/// A run with one player standing still on the floor, carrying `kind`.
fn armed_with(kind: AttackKind, level: u8) -> Game {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    clear_arena(&mut game);
    game.players[0].attack = kind;
    game.players[0].attack_level = level;
    game.players[0].facing_right = true;
    game
}

fn attack_input() -> InputFrame {
    let mut input = idle();
    input.players[0].attack = true;
    input
}

#[test]
fn a_shot_is_square_and_leaves_the_player() {
    let mut game = armed_with(AttackKind::Bullet, 1);
    let from = game.players[0].body.x;
    game.tick(&attack_input());

    assert_eq!(game.bullets.len(), 1, "the press should have thrown one");
    let b = game.bullets[0];
    assert!((b.body.w - b.body.h).abs() < 0.01, "a bullet is a square in pixels");
    assert!(b.body.x > from, "and it starts on the side the player faces");

    let start = game.bullets[0].body.x;
    clear_arena(&mut game);
    game.tick(&idle());
    let step = game.bullets[0].body.x - start;
    let walk = game.viewport.wper(PLAYER_MOVE_PCT);
    assert!(
        (step - walk * BULLET_SPEED_MULT).abs() < 0.01,
        "it should travel at four walks a tick, moved {step}"
    );
}

#[test]
fn a_shot_that_hits_nothing_stops_at_the_edge_of_the_view() {
    // The field runs forever; without this a miss would fly until the run did.
    let mut game = armed_with(AttackKind::Bullet, 1);
    game.tick(&attack_input());
    assert_eq!(game.bullets.len(), 1);

    for _ in 0..200 {
        clear_arena(&mut game);
        game.tick(&idle());
        if game.bullets.is_empty() {
            return;
        }
    }
    panic!("the bullet never expired");
}

#[test]
fn a_shot_stops_on_what_it_reaches_and_lands_one_tick() {
    let mut game = armed_with(AttackKind::Bullet, 1);
    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(25.0);
    z.hp = 1_000_000.0;
    z.hpmax = 1_000_000.0;
    let before = z.hp;
    game.zombies.push(z);

    game.tick(&attack_input());
    for _ in 0..30 {
        game.flyers.clear();
        game.tick(&idle());
        if game.bullets.is_empty() {
            break;
        }
    }

    // One connecting tick, not the three the shot stays out for: the throw
    // carries the target clear immediately, exactly as it does with a swing.
    // The remaining ticks are not wasted - the stopped shot still catches
    // whatever walks into it - but they are not spent on the first target.
    let dealt = before - game.zombies[0].hp;
    assert_eq!(dealt, 64.0, "one connecting tick");
    assert!(
        dealt < 64.0 * BULLET_HIT_TICKS as f32,
        "the throw should be cutting the shot short"
    );
}

#[test]
fn a_shot_throws_its_target_the_way_a_swing_does() {
    let mut game = armed_with(AttackKind::Bullet, 1);
    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(25.0);
    z.hp = 1_000_000.0;
    z.hpmax = 1_000_000.0;
    game.zombies.push(z);

    game.tick(&attack_input());
    let mut thrown = (false, false);
    for _ in 0..30 {
        game.flyers.clear();
        game.tick(&idle());
        if game.zombies[0].ay < 0.0 {
            thrown.0 = true;
        }
        if game.zombies[0].ax > 0.0 {
            thrown.1 = true;
        }
        if thrown.0 && thrown.1 {
            break;
        }
    }
    assert!(thrown.0, "a shot should throw its target upward");
    assert!(thrown.1, "and away from the shooter");
}

#[test]
fn the_level_caps_how_many_shots_are_out_at_once() {
    for level in 1..=3u8 {
        let mut game = armed_with(AttackKind::Bullet, level);
        // Press on every tick the cooldown allows; the cap is what stops it.
        for _ in 0..40 {
            clear_arena(&mut game);
            game.tick(&attack_input());
            assert!(
                game.bullets.len() <= level as usize,
                "level {level} let {} out at once",
                game.bullets.len()
            );
        }
    }
}

#[test]
fn a_held_button_keeps_shooting() {
    // The thrown attack is the only kind that repeats: every other one wants
    // exactly one swing per press, or a combo could never be broken out of.
    let mut game = armed_with(AttackKind::Bullet, 3);
    let mut held = idle();
    held.players[0].attack_held = true;

    let mut fired = 0;
    for _ in 0..90 {
        let before = game.bullets.len();
        game.zombies.clear();
        game.flyers.clear();
        game.tick(&held);
        if game.bullets.len() > before {
            fired += 1;
        }
    }
    assert!(fired > 1, "holding the button only produced {fired} shot(s)");
}

#[test]
fn a_held_button_does_not_keep_swinging() {
    let mut game = armed_with(AttackKind::Basic, 1);
    let mut held = idle();
    held.players[0].attack_held = true;

    let mut swings = 0;
    let mut was_swinging = false;
    for _ in 0..90 {
        game.zombies.clear();
        game.flyers.clear();
        game.tick(&held);
        let now = game.players[0].attacking();
        if now && !was_swinging {
            swings += 1;
        }
        was_swinging = now;
    }
    assert_eq!(swings, 0, "a held button swung {swings} times on its own");
}

#[test]
fn traps_are_placed_as_fast_as_the_button_is_pressed() {
    // No wait between placements: how many may stand at once is the only limit.
    let mut game = armed_with(AttackKind::Frozen, 3);
    for _ in 0..3 {
        clear_arena(&mut game);
        game.tick(&attack_input());
    }
    assert_eq!(game.traps.len(), 3, "three presses on three ticks place three");
}

#[test]
fn a_new_trap_replaces_the_only_one_at_the_first_level() {
    let mut game = armed_with(AttackKind::Frozen, 1);
    game.tick(&attack_input());
    let first = game.traps[0].body.x;

    // Walk a little so the second lands somewhere else.
    let v = game.viewport;
    game.players[0].body.x += v.wper(20.0);
    clear_arena(&mut game);
    game.tick(&attack_input());

    assert_eq!(game.traps.len(), 1, "level one holds exactly one");
    assert!(
        (game.traps[0].body.x - first).abs() > v.wper(10.0),
        "the standing one should be the new one"
    );
}

#[test]
fn a_full_set_of_traps_loses_its_oldest_to_the_next_one() {
    let mut game = armed_with(AttackKind::Frozen, 3);
    let v = game.viewport;

    let mut placed = Vec::new();
    for _ in 0..3 {
        clear_arena(&mut game);
        game.tick(&attack_input());
        placed.push(game.traps.last().unwrap().body.x);
        game.players[0].body.x += v.wper(15.0);
    }
    assert_eq!(game.traps.len(), 3);

    // The fourth press still places, and the first one is what makes room.
    clear_arena(&mut game);
    game.tick(&attack_input());
    assert_eq!(game.traps.len(), 3, "the cap still holds");
    assert!(
        !game.traps.iter().any(|t| (t.body.x - placed[0]).abs() < 0.01),
        "the oldest should have made way"
    );
    assert!(
        game.traps.iter().any(|t| (t.body.x - placed[1]).abs() < 0.01),
        "the second oldest should still be standing"
    );
}

#[test]
fn a_shot_already_in_the_air_is_not_recalled_to_make_room() {
    // The two detached kinds differ here on purpose: a placed box is the
    // player's to move, a bullet has left.
    let mut game = armed_with(AttackKind::Bullet, 1);
    game.tick(&attack_input());
    assert_eq!(game.bullets.len(), 1);
    let first = game.bullets[0].body.x;

    clear_arena(&mut game);
    game.tick(&attack_input());
    assert_eq!(game.bullets.len(), 1, "the cap still holds");
    assert!(
        (game.bullets[0].body.x - first).abs() > 0.0,
        "and the one flying is still the first"
    );
}

#[test]
fn a_trap_stands_where_the_swing_would_have_reached() {
    let mut game = armed_with(AttackKind::Frozen, 1);
    let player_right = game.players[0].body.x + game.players[0].body.w;
    game.tick(&attack_input());

    assert_eq!(game.traps.len(), 1);
    let t = game.traps[0];
    assert!(t.body.x >= player_right - 0.01, "placed in front, not on the player");
    assert!(t.ticks_left > 0);
}

#[test]
fn a_trap_expires_so_it_cannot_be_parked_as_a_shield() {
    let mut game = armed_with(AttackKind::Frozen, 1);
    game.tick(&attack_input());
    assert_eq!(game.traps.len(), 1);

    for _ in 0..TRAP_LIFE_TICKS + 2 {
        clear_arena(&mut game);
        // Deliberately idle: a trap must go on its own, not only when replaced.
        game.tick(&idle());
    }
    assert!(game.traps.is_empty(), "the trap outlived its welcome");
}

#[test]
fn a_trap_throws_what_it_catches() {
    // It holds ground the same way a swing does, and its life is shorter than
    // the arc it throws into - so nothing it catches comes back down inside it
    // and gets juggled.
    let mut game = armed_with(AttackKind::Frozen, 1);
    game.tick(&attack_input());
    let trap = game.traps[0].body;

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body = trap;
    z.hp = 1_000_000.0;
    z.hpmax = 1_000_000.0;
    game.zombies.push(z);
    game.flyers.clear();
    game.tick(&idle());

    assert!(game.zombies[0].ay < 0.0, "a trap should throw like a swing");
    assert!(
        (TRAP_LIFE_TICKS as f32) < 40.0,
        "a trap that outlasts the throw arc would juggle what it catches"
    );
}

#[test]
fn a_trap_grinds_rather_than_deleting_what_walks_in() {
    // It never throws, so anything standing in it takes every tick. That is
    // why its per-tick damage is a quarter of a swing's: at full value a single
    // trap would be a kill zone rather than ground held.
    let mut game = armed_with(AttackKind::Frozen, 1);
    game.tick(&attack_input());
    let trap = game.traps[0].body;

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body = trap;
    z.hp = 1_000_000.0;
    z.hpmax = 1_000_000.0;
    let before = z.hp;
    game.zombies.push(z);

    game.flyers.clear();
    game.tick(&idle());
    let one_tick = before - game.zombies[0].hp;
    assert_eq!(one_tick, 16.0, "a quarter of a swing's tick");
}

#[test]
fn a_detached_kind_never_holds_a_swing_out() {
    // The box on the player stays the resting one: there is nothing held, and
    // the thing that does the work is in the world.
    for kind in [AttackKind::Bullet, AttackKind::Frozen] {
        let mut game = armed_with(kind, 1);
        let v = game.viewport;
        game.players[0].update_gun(&v, 1.0, game.wave);
        let resting = game.players[0].gun;

        game.tick(&attack_input());
        assert_eq!(
            game.players[0].gun.w, resting.w,
            "{kind:?} held a swing out anyway"
        );
        assert!(
            game.players[0].gun_back.is_none(),
            "{kind:?} put out a second box"
        );
    }
}

/* ---------------- dash ---------------- */

fn dash_input() -> InputFrame {
    let mut input = idle();
    input.players[0].dash = true;
    input
}

/// A run with the player standing still on the floor, facing right.
fn ready_to_dash(game: &mut Game) {
    game.start_run(1);
    settle_on_floor(game);
    game.players[0].facing_right = true;
    game.players[0].dash_cooldown = 0;
    // Enough to pay without being enough to buy a charge on the way past.
    game.players[0].energy = game.players[0].energy_needed(game.wave) - 1;
}

#[test]
fn a_dash_never_steps_further_than_a_body() {
    // The shove is a per-tick overlap test, so a step longer than an enemy is
    // wide would carry the player clean through one without ever touching it.
    // This is what pins DASH_TICKS to its floor.
    let v = Viewport::new(1280.0, 800.0);
    let step = v.wper(DASH_DISTANCE_PCT) / DASH_TICKS as f32;
    let body = v.wper(5.0);
    assert!(step <= body, "a step of {step} skips past a {body}-wide enemy");
}

#[test]
fn a_dash_carries_the_player_the_whole_distance() {
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);

    let from = game.players[0].body.x;
    game.tick(&dash_input());
    for _ in 0..DASH_TICKS {
        clear_arena(&mut game);
        game.tick(&idle());
    }
    let travelled = game.players[0].body.x - from;
    let want = game.viewport.wper(DASH_DISTANCE_PCT);
    assert!(
        (travelled - want).abs() < game.viewport.wper(2.0),
        "dashed {travelled}, wanted about {want}"
    );
}

#[test]
fn the_cooldown_locks_out_a_second_dash() {
    let mut game = new_game();
    ready_to_dash(&mut game);
    game.players[0].energy = 100_000; // never the reason it refuses
    clear_arena(&mut game);

    game.tick(&dash_input());
    assert!(game.players[0].dashing());

    // Let the dash run its course.
    for _ in 0..DASH_TICKS {
        clear_arena(&mut game);
        game.tick(&idle());
    }
    assert!(!game.players[0].dashing(), "the dash should be over");
    assert!(game.players[0].dash_cooldown > 0, "and the cooldown should be running");

    // Pressing inside the cooldown is refused, however hard.
    while game.players[0].dash_cooldown > 0 {
        clear_arena(&mut game);
        game.tick(&dash_input());
        assert!(!game.players[0].dashing(), "the cooldown let a dash through");
    }

    // And the first press after it works.
    clear_arena(&mut game);
    game.tick(&dash_input());
    assert!(game.players[0].dashing(), "it should go again once the cooldown is out");
}

#[test]
fn a_dash_costs_a_third_of_a_bar() {
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);

    let needed = game.players[0].energy_needed(game.wave);
    let before = game.players[0].energy;
    let want_cost = (needed + 2) / 3;

    game.tick(&dash_input());
    assert_eq!(before - game.players[0].energy, want_cost);
}

#[test]
fn an_empty_bar_breaks_a_charge_without_discounting_the_next_one() {
    // Breaking a charge has to count as spending one. The next charge is priced
    // on how many are held, so quietly dropping one would make the fallback
    // cheaper than paying properly.
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);
    game.players[0].energy = 0;
    game.players[0].super_charges = 1;
    game.players[0].attacks_since_power_up = 0;
    let price_before = game.players[0].energy_needed(game.wave);

    game.tick(&dash_input());

    assert!(game.players[0].dashing(), "a held charge should have paid for it");
    assert_eq!(game.players[0].super_charges, 0);
    assert_eq!(
        game.players[0].energy_needed(game.wave),
        price_before,
        "breaking a charge must not discount the next one"
    );
    // The change from the broken charge stays on the bar.
    assert_eq!(game.players[0].energy, price_before - (price_before + 2) / 3);
}

#[test]
fn a_dash_with_nothing_to_pay_with_does_nothing() {
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);
    game.players[0].energy = 0;
    game.players[0].super_charges = 0;

    let from = game.players[0].body.x;
    game.tick(&dash_input());
    assert!(!game.players[0].dashing());
    assert!((game.players[0].body.x - from).abs() < 1.0, "it moved anyway");
}

#[test]
fn a_dash_throws_enemies_clear_and_hurts_nobody() {
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(10.0);
    let hp_before = z.hp;
    game.zombies.push(z);
    game.flyers.clear();
    let player_hp = game.players[0].hp;

    // Long enough for the dash to reach it.
    game.tick(&dash_input());
    for _ in 0..DASH_TICKS {
        game.tick(&idle());
    }

    let z = &game.zombies[0];
    assert!(z.ay < 0.0, "the enemy should have been thrown upward");
    assert_eq!(z.hp, hp_before, "a dash must not damage what it shoves");
    assert_eq!(game.players[0].hp, player_hp, "and must not cost the player health");
}

#[test]
fn the_shove_outlasts_the_dash_by_three_frames() {
    // The travel stops on a frame, but the player is left standing inside the
    // crowd it went through. Without the tail the last enemy shoved would be
    // free to touch back on the very next tick.
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);

    game.tick(&dash_input());
    while game.players[0].dashing() {
        clear_arena(&mut game);
        game.tick(&idle());
    }

    // Park an enemy on top of the player each tick and see whether it gets
    // shoved or whether it bites.
    let mut safe_frames = 0;
    for _ in 0..(DASH_GRACE_TICKS + 3) {
        clear_arena(&mut game);
        let v = game.viewport;
        let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
        z.body = game.players[0].body;
        game.zombies.push(z);
        let hp_before = game.players[0].hp;

        game.tick(&idle());

        if game.players[0].hp < hp_before {
            break;
        }
        safe_frames += 1;
    }
    assert_eq!(
        safe_frames, DASH_GRACE_TICKS,
        "the window should outlast the travel by exactly {DASH_GRACE_TICKS} frames"
    );
}

#[test]
fn a_husk_still_hurts_a_dashing_player() {
    // The husk is the one thing a dash cannot shove: it takes no damage and
    // does not move, which makes it the counter to the crowd-clear. Everything
    // else on the field a dash goes through untouched, hazard included would
    // leave nothing a dash could not simply drive over.
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);
    game.flyers.clear();

    let v = game.viewport;
    let mut parent = Zombie::shedder(&v, &mut game.rng);
    parent.body.x = game.players[0].body.x + v.wper(200.0);
    let mut husk = game.players[0].body;
    husk.x = game.players[0].body.x + v.wper(10.0);
    parent.husks.push(husk);
    game.zombies.push(parent);

    let hp_before = game.players[0].hp;
    game.tick(&dash_input());
    for _ in 0..DASH_TICKS {
        game.tick(&idle());
    }
    assert!(game.players[0].hp < hp_before, "a husk should still bite mid-dash");
}

#[test]
fn the_melee_box_is_red_when_the_dash_is_spent_and_white_when_ready() {
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);

    let ready = game.players[0].gun_color();
    assert_eq!(ready, crate::color::Rgb::new(255.0, 255.0, 255.0), "a ready dash reads white");
    assert!(!game.players[0].attacking(), "and it is the idle box being read");

    game.tick(&dash_input());
    for _ in 0..DASH_TICKS {
        clear_arena(&mut game);
        game.tick(&idle());
    }
    let spent = game.players[0].gun_color();
    assert!(spent.g < 32.0 && spent.b < 32.0, "it should be red the moment the dash ends");

    // And it walks back to white rather than snapping.
    let mut previous = spent.g;
    for _ in 0..DASH_COOLDOWN_TICKS {
        clear_arena(&mut game);
        game.tick(&idle());
        let now = game.players[0].gun_color().g;
        assert!(now >= previous, "the indicator went backwards");
        previous = now;
    }
    assert_eq!(game.players[0].gun_color(), crate::color::Rgb::new(255.0, 255.0, 255.0));
}

#[test]
fn a_swing_is_always_white_however_spent_the_dash_is() {
    // Mid-attack the box is what the player reads for reach. Recolouring it
    // then would say two things with one shape at the moment it matters most.
    let mut game = new_game();
    ready_to_dash(&mut game);
    clear_arena(&mut game);

    game.tick(&dash_input());
    for _ in 0..DASH_TICKS {
        clear_arena(&mut game);
        game.tick(&idle());
    }
    assert!(game.players[0].dash_cooldown > 0, "the dash should be on cooldown");
    assert!(game.players[0].gun_color().g < 64.0, "idle, it should be showing red");

    game.players[0].attack_ticks = ATTACK_TICKS;
    assert_eq!(
        game.players[0].gun_color(),
        crate::color::Rgb::new(255.0, 255.0, 255.0),
        "a swing must stay white even with the dash spent"
    );
}

/* ---------------- the developer menu ---------------- */

fn dev_chord() -> InputFrame {
    InputFrame { dev_menu: true, ..Default::default() }
}

#[test]
fn the_dev_menu_opens_from_the_title_and_nowhere_else() {
    let mut game = new_game();
    game.tick(&dev_chord());
    assert_eq!(game.state, State::DevMenu, "the chord should open it from the title");

    // Not from inside settings, where it would be a surprise.
    let mut game = new_game();
    game.state = State::Settings;
    game.tick(&dev_chord());
    assert_eq!(game.state, State::Settings);

    // Nor from a run: the chord is Select and Down on a PSP, and Down is a
    // button the player is holding all the time.
    let mut game = new_game();
    game.start_run(1);
    game.tick(&dev_chord());
    assert_eq!(game.state, State::Playing);
}

#[test]
fn a_dev_run_starts_on_the_wave_and_score_it_was_given() {
    let mut game = new_game();
    game.dev.wave = 17;
    game.dev.score = 12_500;
    game.dev.players = 1;
    game.start_dev_run();

    assert_eq!(game.state, State::Playing);
    assert_eq!(game.wave, 17);
    assert_eq!(game.total_score(), 12_500, "the team total should be what the menu showed");
}

#[test]
fn the_score_row_steps_by_five_hundred_and_stops_at_both_ends() {
    let mut dev = DevSetup::default();
    dev.adjust_score(1);
    assert_eq!(dev.score, DEV_SCORE_STEP);
    dev.adjust_score(1);
    assert_eq!(dev.score, DEV_SCORE_STEP * 2);
    dev.adjust_score(-1);
    assert_eq!(dev.score, DEV_SCORE_STEP);

    for _ in 0..5 {
        dev.adjust_score(-1);
    }
    assert_eq!(dev.score, 0, "score should not go negative");

    for _ in 0..5000 {
        dev.adjust_score(1);
    }
    assert_eq!(dev.score, DEV_MAX_SCORE, "score should stop at its ceiling");
}

#[test]
fn the_wave_row_stops_at_one() {
    let mut dev = DevSetup::default();
    for _ in 0..5 {
        dev.adjust_wave(-1);
    }
    assert_eq!(dev.wave, 1);
    for _ in 0..500 {
        dev.adjust_wave(1);
    }
    assert_eq!(dev.wave, DEV_MAX_WAVE);
}

#[test]
fn the_modifier_rows_cycle_back_through_any() {
    let mut dev = DevSetup::default();
    assert_eq!(dev.kind, None, "they start unpinned");
    assert_eq!(dev.kind_label(), "ANY");

    let mut seen_pinned = false;
    for _ in 0..6 {
        dev.cycle_kind(1);
        seen_pinned |= dev.kind.is_some();
    }
    assert!(seen_pinned, "cycling never pinned anything");
    assert_eq!(dev.kind, None, "a full cycle should come back to ANY");

    // Backwards too, and the labels never come out blank.
    dev.cycle_rule(-1);
    assert!(dev.rule.is_some());
    assert!(!dev.rule_label().is_empty());
}

#[test]
fn pinned_modifiers_hold_for_every_wave_of_the_run() {
    let mut game = new_game();
    game.dev.wave = 3;
    game.dev.kind = Some(WaveKind::FlyersOnly);
    game.dev.rule = Some(WaveRule::NoJumps);
    game.start_dev_run();

    assert_eq!(game.waves.kind, WaveKind::FlyersOnly);
    assert_eq!(game.waves.rule, WaveRule::NoJumps);

    // Every later wave settles the same way, however the dice fall.
    for wave in 4..30 {
        game.waves.begin_wave(wave, &mut game.rng);
        assert_eq!(game.waves.kind, WaveKind::FlyersOnly, "wave {wave} lost the pin");
        assert_eq!(game.waves.rule, WaveRule::NoJumps, "wave {wave} lost the pin");
    }
}

#[test]
fn an_ordinary_run_never_inherits_the_pins() {
    let mut game = new_game();
    game.dev.kind = Some(WaveKind::BasicOnly);
    game.dev.rule = Some(WaveRule::Hidden);
    game.start_dev_run();
    assert_eq!(game.waves.forced_kind, Some(WaveKind::BasicOnly));

    // Back to the menu, then a normal start.
    game.start_run(1);
    assert_eq!(game.waves.forced_kind, None, "a normal run kept a pinned kind");
    assert_eq!(game.waves.forced_rule, None, "a normal run kept a pinned rule");
}

#[test]
fn the_dev_menu_hands_out_an_attack_and_its_level() {
    let mut game = new_game();
    game.dev.attack = AttackKind::Frozen;
    game.dev.attack_level = 3;
    game.dev.players = 1;
    game.start_dev_run();

    assert_eq!(game.players[0].attack, AttackKind::Frozen);
    assert_eq!(game.players[0].attack_level, 3);
    // And the level is what the cap actually reads. Counted as a high-water
    // mark rather than a snapshot: traps expire, so what is standing at any
    // one moment depends on when you look.
    let mut most = 0;
    for _ in 0..40 {
        clear_arena(&mut game);
        game.tick(&attack_input());
        most = most.max(game.traps.len());
        assert!(game.traps.len() <= 3, "level three let a fourth out");
    }
    assert_eq!(most, 3, "three traps at level three");
}

#[test]
fn an_ordinary_run_starts_on_the_basic_attack() {
    // The dev menu must not leak a weapon into a run started the normal way.
    let mut game = new_game();
    game.dev.attack = AttackKind::Hammer;
    game.dev.attack_level = 3;
    game.start_dev_run();
    assert_eq!(game.players[0].attack, AttackKind::Hammer);

    game.start_run(1);
    assert_eq!(game.players[0].attack, AttackKind::Basic);
    assert_eq!(game.players[0].attack_level, 1);
}

#[test]
fn the_attack_row_cycles_through_every_kind_and_back() {
    let mut dev = DevSetup::default();
    assert_eq!(dev.attack, AttackKind::Basic, "it starts where the game does");

    // The list is the upgrade roster plus the attack the game starts on, so a
    // full turn is one step longer than ALL.
    let mut seen = Vec::new();
    for _ in 0..=AttackKind::ALL.len() {
        seen.push(dev.attack);
        dev.cycle_attack(1);
    }
    for kind in AttackKind::ALL {
        assert!(seen.contains(&kind), "{kind:?} is never offered");
    }
    assert!(seen.contains(&AttackKind::Basic), "nor is the plain one");
    assert_eq!(dev.attack, AttackKind::Basic, "a full cycle comes home");

    dev.adjust_attack_level(-5);
    assert_eq!(dev.attack_level, 1, "level should not go below one");
    dev.adjust_attack_level(99);
    assert_eq!(dev.attack_level, MAX_LEVEL);
}

#[test]
fn down_still_brings_you_down_on_a_wave_with_no_jumping() {
    // The player did not choose to be up there - a hit put them there - and
    // with the wall raised from standing instead, down had nothing left to do.
    let mut game = player_with(Boons::default());
    game.state = State::Playing;
    game.waves.rule = WaveRule::NoJumps;
    let v = game.viewport;

    let drop_from = |game: &mut Game, slam: bool| {
        game.players[0].body.y = v.hper(30.0);
        game.players[0].ay = 0.0;
        game.players[0].grounded = false;
        let mut frame = idle();
        frame.players[0].slam = slam;
        game.tick(&frame);
        game.players[0].ay
    };

    let drifting = drop_from(&mut game, false);
    let slammed = drop_from(&mut game, true);
    assert!(
        slammed > drifting,
        "down should hurry the fall: {slammed} against {drifting}"
    );
}

#[test]
fn coming_down_on_a_wave_with_no_jumping_cannot_be_punished() {
    // Down is the only way back to the floor there, and the floor is where the
    // wall goes up. A descent that could be hit on the way would make the one
    // recovery that wave offers a worse idea than drifting.
    let mut game = player_with(Boons::default());
    game.state = State::Playing;
    game.waves.rule = WaveRule::NoJumps;
    let v = game.viewport;
    game.players[0].body.y = v.hper(30.0);
    game.players[0].grounded = false;

    let mut frame = idle();
    frame.players[0].slam = true;
    game.tick(&frame);

    assert!(game.players[0].field.readiness, "the wall should be on its way");
    assert!(
        game.players[0].untouchable(game.timer, Reach::Normal),
        "and the fall should be covered"
    );

    let full = game.players[0].hp;
    let source = Body::new(0.0, 0.0, 10.0, 10.0);
    game.damage_player(0, 40.0, &source, Reach::Normal);
    assert_eq!(game.players[0].hp, full, "something reached it on the way down");
}

#[test]
fn the_landing_on_a_wave_with_no_jumping_still_puts_the_wall_up() {
    // The cover is not free: it is the wall being raised, and it is paid for
    // out of the same charge raising one from standing costs.
    let mut game = player_with(Boons::default());
    game.state = State::Playing;
    game.waves.rule = WaveRule::NoJumps;
    let v = game.viewport;
    game.players[0].super_charges = 1;
    game.players[0].body.y = v.hper(30.0);
    game.players[0].grounded = false;

    let mut frame = idle();
    frame.players[0].slam = true;
    game.tick(&frame);
    for _ in 0..60 {
        game.tick(&idle());
        if game.players[0].field.active {
            break;
        }
    }
    assert!(game.players[0].field.active, "it never went up");
    assert_eq!(game.players[0].super_charges, 0, "and it should have been paid for");
    assert!(!game.players[0].field.readiness, "the intent is spent either way");
}

#[test]
fn a_landing_with_nothing_to_pay_with_does_not_stay_covered() {
    // Otherwise pressing down on an empty bar would buy permanent cover: the
    // intent has to be spent whether or not it bought anything.
    let mut game = player_with(Boons::default());
    game.state = State::Playing;
    game.waves.rule = WaveRule::NoJumps;
    let v = game.viewport;
    game.players[0].super_charges = 0;
    game.players[0].body.y = v.hper(30.0);
    game.players[0].grounded = false;

    let mut frame = idle();
    frame.players[0].slam = true;
    game.tick(&frame);
    for _ in 0..60 {
        game.tick(&idle());
    }
    assert!(!game.players[0].field.readiness, "it stayed armed with nothing to arm");
    assert!(!game.players[0].field.active);
    assert!(
        !game.players[0].untouchable(game.timer, Reach::Normal),
        "and so it should be touchable again"
    );
}

#[test]
fn a_wave_with_no_jumping_still_will_not_let_you_jump() {
    // Giving down its job back must not give the button above it one.
    let mut game = player_with(Boons { double_jump: true, ..Default::default() });
    game.state = State::Playing;
    game.waves.rule = WaveRule::NoJumps;
    let floor = game.players[0].body.y;
    let mut frame = idle();
    frame.players[0].jump = true;
    for _ in 0..4 {
        game.tick(&frame);
    }
    assert!(game.players[0].body.y >= floor, "it left the floor anyway");
}

#[test]
fn the_dev_menu_can_hand_out_every_upgrade_at_once() {
    // Reaching one in a real run costs tens of thousands of points, and most of
    // what wants testing wants all of them together.
    let mut dev = DevSetup::default();
    assert!(!dev.all_boons());

    dev.toggle_all_boons();
    assert!(dev.all_boons(), "one press should turn the lot on");
    assert!(dev.boons.double_jump && dev.boons.dash_free && dev.boons.shield);
    assert_ne!(
        dev.boons.wall,
        WallMod::Plain,
        "the wall should go to a modified one - plain is what off looks like"
    );

    dev.toggle_all_boons();
    assert_eq!(dev.boons, Boons::default(), "and the next press should clear it");
}

#[test]
fn each_upgrade_can_also_be_set_on_its_own() {
    let mut dev = DevSetup::default();
    dev.toggle_double_jump();
    assert!(dev.boons.double_jump);
    assert!(!dev.boons.dash_free && !dev.boons.shield, "only the one asked for");
    assert!(!dev.all_boons(), "one of four is not all of them");

    dev.toggle_dash_free();
    dev.toggle_shield();
    assert!(!dev.all_boons(), "the wall is one of them too");
    dev.cycle_wall(1);
    assert!(dev.all_boons());

    dev.toggle_double_jump();
    assert!(!dev.boons.double_jump, "a switch goes both ways");
}

#[test]
fn the_wall_cycles_through_all_three_and_comes_back() {
    let mut dev = DevSetup::default();
    let mut seen = Vec::new();
    for _ in 0..3 {
        dev.cycle_wall(1);
        seen.push(dev.boons.wall);
    }
    assert!(seen.contains(&WallMod::Pull) && seen.contains(&WallMod::Push));
    assert_eq!(dev.boons.wall, WallMod::Plain, "three steps should be a full turn");
    dev.cycle_wall(-1);
    assert_eq!(dev.boons.wall, WallMod::Push, "and it should turn the other way too");
}

#[test]
fn a_dev_run_starts_holding_what_the_menu_showed() {
    let mut game = new_game();
    game.dev.players = 2;
    game.dev.toggle_all_boons();
    let wanted = game.dev.boons;
    game.start_dev_run();

    assert_eq!(game.players.len(), 2);
    for p in game.players.iter() {
        assert_eq!(p.boons, wanted, "every player should start with them");
    }
}

#[test]
fn an_ordinary_run_still_starts_with_nothing() {
    // The dev menu pins these for its own runs only; the title screen must not
    // inherit whatever was left set there.
    let mut game = new_game();
    game.dev.toggle_all_boons();
    game.start_run(1);
    assert_eq!(game.players[0].boons, Boons::default());
}

#[test]
fn the_dev_menu_fits_on_a_psp_screen() {
    // The dev menu has more rows than any other and keeps growing - every
    // parameter worth starting a run with ends up here. At the usual pitch they
    // run off the bottom of a 480x272 panel, under the floor band, which is why
    // this menu carries a pitch of its own.
    let game = single_player_platform();
    let rows = game.menu_rows([true, false]).len();
    let last = game.dev_menu.row_y_pct(rows - 1);
    assert!(
        game.state == State::Title || last < GROUND_Y_PCT,
        "the last row would land at {last}%, under the floor"
    );

    let mut game = single_player_platform();
    game.state = State::DevMenu;
    let rows = game.menu_rows([true, false]).len();
    let last = game.dev_menu.row_y_pct(rows - 1);
    assert!(last < GROUND_Y_PCT, "the last of {rows} rows lands at {last}%");
}

#[test]
fn a_dev_run_honours_the_platform_player_limit() {
    // A PSP seats one, so asking for two must not produce a second player who
    // has no controller.
    let mut game = single_player_platform();
    game.dev.players = 2;
    game.start_dev_run();
    assert_eq!(game.players.len(), 1);
}

/* ---------------- the shedder ---------------- */

/// Drops a shedder on the ground just to the right of player 0 and returns the
/// body it is standing in.
fn plant_shedder(game: &mut Game) -> Body {
    let v = game.viewport;
    let mut z = Zombie::shedder(&v, &mut game.rng);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(6.0);
    let body = z.body;
    game.zombies.push(z);
    body
}

fn the_shedder(game: &Game) -> &Zombie {
    game.zombies
        .iter()
        .find(|z| z.behaviors.shed)
        .expect("the shedder is gone")
}

#[test]
fn the_shedder_waits_for_its_wave() {
    let mut game = new_game();
    for wave in 1..SHEDDER_MIN_WAVE {
        for _ in 0..500 {
            assert_ne!(
                pick_ground(wave, &mut game.rng),
                GroundKind::Shedder,
                "a shedder turned up on wave {wave}"
            );
        }
    }
    let mut seen = false;
    for _ in 0..2000 {
        if pick_ground(SHEDDER_MIN_WAVE, &mut game.rng) == GroundKind::Shedder {
            seen = true;
            break;
        }
    }
    assert!(seen, "the shedder never turned up on its own wave");
}

#[test]
fn a_hurt_shedder_lands_beyond_the_longest_reach() {
    // The whole point of it: a hit cannot be followed up, however far the
    // player's reach has ramped.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    let body = plant_shedder(&mut game);

    swing_at(&mut game, 0, body);
    game.tick(&idle());

    let v = game.viewport;
    let player = game.players[0].body.center_x();
    let gap = (the_shedder(&game).body.center_x() - player).abs();
    assert!(
        gap > v.wper(GUN_MAX_REACH_PCT),
        "it landed {gap} away, inside the {} ceiling",
        v.wper(GUN_MAX_REACH_PCT)
    );
}

#[test]
fn a_hurt_shedder_leaves_a_husk_where_the_hit_landed() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    let body = plant_shedder(&mut game);

    swing_at(&mut game, 0, body);
    game.tick(&idle());

    let husks = &the_shedder(&game).husks;
    assert_eq!(husks.len(), 1, "a hit should leave exactly one husk");
    assert!(
        (husks[0].x - body.x).abs() < 1.0 && (husks[0].y - body.y).abs() < 1.0,
        "the husk is not where the hit was"
    );
}

#[test]
fn an_ordinary_shedder_keeps_only_the_last_husk() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    plant_shedder(&mut game);

    for _ in 0..4 {
        let body = the_shedder(&game).body;
        // Chase it down: it is out of reach after every hit.
        game.players[0].body.x = body.x - game.players[0].body.w;
        swing_at(&mut game, 0, body);
        game.tick(&idle());
        assert_eq!(
            the_shedder(&game).husks.len(),
            SHEDDER_HUSKS,
            "an ordinary shedder should replace its husk, not add to it"
        );
    }
}

#[test]
fn a_husk_hurts_on_contact() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    plant_shedder(&mut game);
    game.flyers.clear();

    let over_player = game.players[0].body;
    let k = game
        .zombies
        .iter()
        .position(|z| z.behaviors.shed)
        .unwrap();
    game.zombies[k].husks.push(over_player);

    let before = game.players[0].hp;
    game.tick(&idle());
    assert!(game.players[0].hp < before, "standing in a husk should hurt");
}

#[test]
fn a_husk_cannot_be_hurt_back() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    plant_shedder(&mut game);

    let v = game.viewport;
    let k = game
        .zombies
        .iter()
        .position(|z| z.behaviors.shed)
        .unwrap();
    let mut husk = game.zombies[k].body;
    husk.x = game.players[0].body.x + v.wper(8.0);
    game.zombies[k].husks.push(husk);
    // The parent goes far off, or the swings below would land on it instead and
    // it would replace the husk under test with a fresh one.
    game.zombies[k].body.x = husk.x + v.wper(300.0);

    for _ in 0..6 {
        swing_at(&mut game, 0, husk);
        game.tick(&idle());
    }
    assert_eq!(
        the_shedder(&game).husks.iter().filter(|h| h.x == husk.x).count(),
        1,
        "swinging at a husk should not remove it"
    );
}

#[test]
fn husks_go_when_their_parent_does() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    let body = plant_shedder(&mut game);

    let k = game
        .zombies
        .iter()
        .position(|z| z.behaviors.shed)
        .unwrap();
    game.zombies[k].husks.push(body);
    // One swing's worth of health left. An enemy is only ever collected inside
    // a hit, so setting health negative on its own would leave it standing.
    game.zombies[k].hp = 1.0;

    swing_at(&mut game, 0, body);
    // The kill is collected on the pass after the one that did the damage.
    game.tick(&idle());
    game.tick(&idle());

    assert!(
        !game.zombies.iter().any(|z| z.behaviors.shed),
        "the shedder should be gone"
    );
    assert!(
        game.zombies.iter().all(|z| z.husks.is_empty()),
        "a husk outlived its parent"
    );
}

#[test]
fn husks_do_not_hold_a_wave_open() {
    // They are scenery, so they must not count towards the live-enemy total
    // the wave manager waits on.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    let body = plant_shedder(&mut game);

    let k = game
        .zombies
        .iter()
        .position(|z| z.behaviors.shed)
        .unwrap();
    for _ in 0..5 {
        game.zombies[k].husks.push(body);
    }
    let live = game.zombies.len() + game.flyers.len();
    assert_eq!(live, 1, "husks are being counted as enemies");
}

#[test]
fn the_shedder_boss_borrows_the_first_boss_health_and_keeps_every_husk() {
    let mut game = new_game();
    let v = game.viewport;
    let boss = Zombie::shedder_boss(&v, &mut game.rng);
    let plain = Zombie::boss(&v, SHEDDER_BOSS_HEALTH_WAVE, &mut game.rng);

    assert!(boss.is_boss);
    assert!(boss.behaviors.shed);
    assert_eq!(boss.hpmax, plain.hpmax, "it should keep the first boss's health");
    assert!(boss.max_husks > SHEDDER_HUSKS, "the boss should not replace its husks");
}

#[test]
fn wave_fifteen_rolls_between_two_bosses() {
    let (mut ground, mut shedder) = (false, false);
    for seed in 0..80u64 {
        match boss_action_seeded(SHEDDER_BOSS_WAVE, seed) {
            Some(WaveAction::SpawnBosses(_)) => ground = true,
            Some(WaveAction::SpawnShedderBoss) => shedder = true,
            other => panic!("wave 15 produced {other:?}"),
        }
        if ground && shedder {
            break;
        }
    }
    assert!(ground && shedder, "wave 15 only ever produced one of the two");
}

/* ---------------- the drift behind the menus ---------------- */

#[test]
fn the_view_drifts_behind_the_menus() {
    // Nothing else moves on the title screen, so without this the parallax
    // skyline is a still image.
    let mut game = new_game();
    let start = game.camera_x;
    for _ in 0..10 {
        game.tick(&idle());
    }
    let moved = (game.camera_x - start).abs();
    let want = 10.0 * game.viewport.wper(PLAYER_MOVE_PCT * MENU_CAMERA_DRIFT);
    assert!(
        (moved - want).abs() < 0.01,
        "drifted {moved}, wanted {want} ({MENU_CAMERA_DRIFT} of a walk)"
    );
}

#[test]
fn the_drift_is_slower_than_a_player_walks() {
    let mut game = new_game();
    let start = game.camera_x;
    game.tick(&idle());
    let drift = (game.camera_x - start).abs();
    let walk = game.viewport.wper(PLAYER_MOVE_PCT);
    assert!(drift < walk, "the menu should drift, not travel");
}

#[test]
fn the_drift_direction_is_redrawn_on_every_arrival() {
    let mut game = new_game();
    let (mut left, mut right) = (false, false);
    for _ in 0..60 {
        // Leaving and coming back is what re-rolls it; the arrival is spotted
        // by comparing against the previous tick's state, so the run has to
        // actually tick before the title can be returned to.
        game.start_run(1);
        game.tick(&idle());
        game.state = State::Title;
        game.tick(&idle());

        let before = game.camera_x;
        game.tick(&idle());
        match game.camera_x - before {
            d if d > 0.0 => right = true,
            d if d < 0.0 => left = true,
            _ => panic!("the view stopped drifting"),
        }
        if left && right {
            break;
        }
    }
    assert!(left && right, "the drift only ever went one way");
}

#[test]
fn the_run_itself_still_follows_the_players() {
    // The drift must not leak into a run, where the camera has a job.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.camera_x = 0.0;
    let want = game.players[0].body.center_x() - game.viewport.w / 2.0;
    for _ in 0..200 {
        clear_arena(&mut game);
        game.tick(&idle());
    }
    assert!(
        (game.camera_x - want).abs() < game.viewport.wper(2.0),
        "the camera did not settle on the player"
    );
}

/* ---------------- knockback ---------------- */

#[test]
fn a_hit_throws_a_ground_enemy_at_walking_pace() {
    // It used to scale with the enemy's own health, so a boss travelled three
    // times as far from a hit as a runt did.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);

    let v = game.viewport;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.body.x = game.players[0].body.x + v.wper(6.0);
    z.hp = 9999.0;
    z.hpmax = 9999.0;
    let body = z.body;
    game.zombies.push(z);
    game.flyers.clear();

    swing_at(&mut game, 0, body);
    game.tick(&idle());

    let thrown = game.zombies[0].ax.abs();
    assert!(
        (thrown - v.wper(PLAYER_MOVE_PCT)).abs() < 0.01,
        "thrown at {thrown}, wanted a walk of {}",
        v.wper(PLAYER_MOVE_PCT)
    );
}

/* ---------------- the background ---------------- */

#[test]
fn the_background_says_whether_a_wave_is_running() {
    let mut game = new_game();
    for _ in 0..120 {
        game.tick(&idle());
    }
    let quiet = game.background.to_rgb();
    assert!(quiet.g > quiet.r, "a quiet screen should be the green one");

    game.start_run(1);
    game.waves.skip_countdown();
    for _ in 0..600 {
        game.tick(&idle());
        if !game.zombies.is_empty() || !game.flyers.is_empty() {
            break;
        }
    }
    for _ in 0..120 {
        game.tick(&idle());
    }
    let fighting = game.background.to_rgb();
    assert!(fighting.r > fighting.g, "a running wave should be the red one");
}

/* ---------------- boons ---------------- */

/// A player standing on the floor with `boons`, mid-run.
fn player_with(boons: Boons) -> Game {
    let mut game = new_game();
    game.start_run(1);
    game.players[0].boons = boons;
    // Settled after the boons are set, not before: the air jump is stocked by
    // standing on the floor, so a player who was already standing when the
    // offer was taken picks it up on the next tick.
    settle_on_floor(&mut game);
    game
}

fn jump_intent() -> InputFrame {
    let mut frame = idle();
    frame.players[0].jump = true;
    frame
}

#[test]
fn without_the_boon_there_is_no_jump_in_the_air() {
    let mut game = player_with(Boons::default());
    game.tick(&jump_intent());
    assert!(!game.players[0].grounded, "the first jump should have left the floor");
    let rising = game.players[0].ay;
    game.tick(&jump_intent());
    assert!(
        game.players[0].ay > rising,
        "a second press in the air should do nothing but let gravity work"
    );
}

#[test]
fn the_boon_buys_exactly_one_jump_off_nothing() {
    let mut game = player_with(Boons { double_jump: true, ..Default::default() });
    game.tick(&jump_intent());
    assert_eq!(game.players[0].air_jumps, 1, "grounded, the stock should be full");

    // Fall for a while, so the second jump is unmistakably a fresh push rather
    // than the first one still going.
    for _ in 0..30 {
        game.tick(&idle());
    }
    let falling = game.players[0].ay;
    assert!(falling > 0.0, "it should be on the way down by now");

    game.tick(&jump_intent());
    assert!(game.players[0].ay < falling, "the air jump should have pushed up");
    assert_eq!(game.players[0].air_jumps, 0, "and there should be none left");

    let rising = game.players[0].ay;
    game.tick(&jump_intent());
    assert!(game.players[0].ay > rising, "a third press buys nothing");
}

#[test]
fn landing_refills_the_air_jump() {
    let mut game = player_with(Boons { double_jump: true, ..Default::default() });
    game.players[0].air_jumps = 0;
    settle_on_floor(&mut game);
    assert_eq!(game.players[0].air_jumps, 1, "the floor should have given it back");
}

#[test]
fn a_wave_that_forbids_jumping_forbids_the_air_jump_too() {
    // A boon does not buy back a system the wave rule turned off.
    let mut game = player_with(Boons { double_jump: true, ..Default::default() });
    game.waves.rule = WaveRule::NoJumps;
    let before = game.players[0].body.y;
    for _ in 0..4 {
        game.tick(&jump_intent());
    }
    assert!(
        game.players[0].body.y >= before,
        "it left the floor on a wave with no jumps"
    );
}

#[test]
fn a_free_dash_skips_the_wait_but_not_the_price() {
    let mut game = player_with(Boons { dash_free: true, ..Default::default() });
    game.players[0].energy = 100_000;
    let mut frame = idle();
    frame.players[0].dash = true;

    game.tick(&frame);
    assert!(game.players[0].dashing(), "the first dash should have started");
    for _ in 0..DASH_TICKS {
        game.tick(&idle());
    }
    assert!(!game.players[0].dashing());
    assert_eq!(
        game.players[0].dash_cooldown, 0,
        "a free dash should not leave the indicator saying wait"
    );

    let before = game.players[0].energy;
    game.tick(&frame);
    assert!(game.players[0].dashing(), "and the next one should start at once");
    assert!(game.players[0].energy < before, "but it is still paid for");
}

#[test]
fn a_free_dash_still_runs_out_of_energy() {
    // Removing the wait leaves the bar as the only gate; without one, the dash
    // would be a permanently open door.
    let mut game = player_with(Boons { dash_free: true, ..Default::default() });
    game.players[0].energy = 0;
    game.players[0].super_charges = 0;
    let mut frame = idle();
    frame.players[0].dash = true;
    game.tick(&frame);
    assert!(!game.players[0].dashing(), "nothing left to pay with");
}

#[test]
fn the_shield_eats_one_touch_then_waits() {
    let mut game = player_with(Boons { shield: true, ..Default::default() });
    let source = Body::new(0.0, 0.0, 10.0, 10.0);
    let full = game.players[0].hp;
    assert!(game.players[0].shield_up(game.timer));

    game.damage_player(0, 40.0, &source, Reach::Normal);
    assert_eq!(game.players[0].hp, full, "the first touch should have been absorbed");
    assert!(!game.players[0].shield_up(game.timer), "and the shield should be down");

    game.damage_player(0, 40.0, &source, Reach::Normal);
    assert!(game.players[0].hp < full, "the second one should land");

    game.timer += SHIELD_COOLDOWN_TICKS;
    assert!(game.players[0].shield_up(game.timer), "a second later it is back");
}

#[test]
fn the_shield_still_throws_the_player_clear() {
    // Absorbing a hit must not also mean standing in it: the throw is what
    // carries the player out of reach of the next one.
    let mut game = player_with(Boons { shield: true, ..Default::default() });
    let source = Body::new(game.players[0].body.x + 200.0, 0.0, 10.0, 10.0);
    game.damage_player(0, 40.0, &source, Reach::Normal);
    assert!(
        game.players[0].knockback_x < 0.0,
        "it should have been thrown away from what touched it"
    );
}

/// Every way of being unable to be hurt, by the name it goes by in play.
fn protections() -> [(&'static str, fn(&mut Game)); 4] {
    [
        ("bought invulnerability", |g: &mut Game| g.players[0].invulnerable = true),
        ("a wall standing", |g: &mut Game| {
            let b = g.players[0].body;
            let v = g.viewport;
            g.players[0].field.activate(&b, &v);
        }),
        ("a wall on the way up", |g: &mut Game| g.players[0].field.readiness = true),
        ("a dash", |g: &mut Game| {
            g.players[0].dash_ticks = DASH_TICKS;
            g.players[0].dash_dir = 1.0;
        }),
    ]
}

#[test]
fn no_protection_lets_anything_through() {
    // The point of putting them in one place: it used to matter *what* was
    // hitting you. A shot and a shedder's hazard do not go through the sites
    // where an enemy body touches the player, so they reached straight through
    // a raised wall and through a dash.
    for (name, arm) in protections() {
        let mut game = player_with(Boons::default());
        arm(&mut game);
        let source = Body::new(0.0, 0.0, 10.0, 10.0);
        let full = game.players[0].hp;
        game.damage_player(0, 40.0, &source, Reach::Normal);
        assert_eq!(game.players[0].hp, full, "{name} let a hit through");
    }
}

#[test]
fn being_untouchable_does_not_spend_the_shield() {
    // A hit that could not have landed anyway must not cost the one that
    // matters later. This is what "fully untouchable" has to mean, or the
    // shield quietly pays for every shot fired at a player behind a wall.
    for (name, arm) in protections() {
        let mut game = player_with(Boons { shield: true, ..Default::default() });
        arm(&mut game);
        let source = Body::new(0.0, 0.0, 10.0, 10.0);
        game.damage_player(0, 40.0, &source, Reach::Normal);
        assert!(
            game.players[0].shield_up(game.timer),
            "{name} spent the shield for nothing"
        );
    }
}

#[test]
fn a_hazard_that_reaches_a_dashing_player_does_spend_the_shield() {
    // It is a real hit, so the shield is what stops it - and pays. Only hits
    // that could not have landed anyway are free.
    let mut game = player_with(Boons { shield: true, ..Default::default() });
    game.players[0].dash_ticks = DASH_TICKS;
    game.players[0].dash_dir = 1.0;
    let source = Body::new(0.0, 0.0, 10.0, 10.0);
    let full = game.players[0].hp;
    game.damage_player(0, 40.0, &source, Reach::ThroughDash);
    assert_eq!(game.players[0].hp, full, "the shield should have taken it");
    assert!(!game.players[0].shield_up(game.timer), "and been spent doing so");
}

#[test]
fn a_shield_alone_still_pays_for_the_hit_it_stops() {
    // The mirror of the above: with nothing else protecting them, the shield is
    // what took the hit, and it should show.
    let mut game = player_with(Boons { shield: true, ..Default::default() });
    let source = Body::new(0.0, 0.0, 10.0, 10.0);
    let full = game.players[0].hp;
    game.damage_player(0, 40.0, &source, Reach::Normal);
    assert_eq!(game.players[0].hp, full, "the shield should have absorbed it");
    assert!(!game.players[0].shield_up(game.timer), "and been spent doing so");
}

#[test]
fn a_dash_is_the_one_protection_that_is_not_thrown_about() {
    // Being untouchable should not mean being immovable - except mid-dash,
    // which is a committed trajectory. A throw there sets `ay` and lifts the
    // player straight out of their own dash.
    let mut game = player_with(Boons::default());
    game.players[0].invulnerable = true;
    let source = Body::new(game.players[0].body.x + 200.0, 0.0, 10.0, 10.0);
    game.damage_player(0, 40.0, &source, Reach::Normal);
    assert!(game.players[0].knockback_x != 0.0, "it should still be shoved");

    let mut game = player_with(Boons::default());
    game.players[0].dash_ticks = DASH_TICKS;
    game.players[0].dash_dir = 1.0;
    let before = game.players[0].ay;
    game.damage_player(0, 40.0, &source, Reach::Normal);
    assert_eq!(game.players[0].knockback_x, 0.0, "a dash should not be shoved");
    assert_eq!(game.players[0].ay, before, "nor lifted out of itself");
}

#[test]
fn a_shot_cannot_reach_through_a_wall_or_a_dash() {
    // Driven through the real projectile path rather than `damage_player`, so
    // the guard is proven where it actually has to hold.
    for (name, arm) in protections() {
        let mut game = player_with(Boons::default());
        let v = game.viewport;
        game.state = State::Playing;
        game.zombies.clear();
        game.flyers.clear();
        // Off the floor: landing is what spends the bought invulnerability, and
        // a player standing on the ground loses it on the first tick.
        game.players[0].body.y -= v.hper(20.0);
        game.players[0].grounded = false;
        arm(&mut game);
        let p = game.players[0].body;
        game.projectiles.push(Projectile {
            // Wide enough that a dash cannot step out of it: what is under test
            // is the guard, not whether the shot connects.
            body: Body::new(p.x - v.wper(10.0), p.y, v.wper(25.0), v.hper(8.0)),
            ax: 0.0,
            ay: 0.0,
            damage: 20.0,
            dead: false,
        });
        let full = game.players[0].hp;
        game.tick(&idle());
        assert_eq!(game.players[0].hp, full, "a shot got through {name}");
    }
}

#[test]
fn a_hazard_reaches_through_a_dash_and_nothing_else() {
    for (name, arm) in protections() {
        let mut game = player_with(Boons::default());
        let v = game.viewport;
        game.state = State::Playing;
        game.zombies.clear();
        game.flyers.clear();
        // Off the floor: landing is what spends the bought invulnerability, and
        // a player standing on the ground loses it on the first tick.
        game.players[0].body.y -= v.hper(20.0);
        game.players[0].grounded = false;
        arm(&mut game);
        let p = game.players[0].body;
        let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
        // Well clear of the player: the husk it left behind is the threat.
        z.body.x = p.x + v.wper(40.0);
        z.max_husks = 4;
        z.husks.push(Body::new(p.x, p.y, p.w, p.h));
        game.zombies.push(z);
        let full = game.players[0].hp;
        game.tick(&idle());
        if name == "a dash" {
            assert!(
                game.players[0].hp < full,
                "the one counter to the crowd-clear stopped working"
            );
        } else {
            assert_eq!(game.players[0].hp, full, "a hazard got through {name}");
        }
    }
}

#[test]
fn boons_belong_to_a_run_and_not_to_a_death_between_waves() {
    let all = Boons { double_jump: true, dash_free: true, shield: true, wall: WallMod::Plain };
    let mut game = player_with(all);
    let v = game.viewport;

    game.players[0].revive(&v, 0.0);
    assert_eq!(game.players[0].boons, all, "dying between waves should not cost them");

    game.start_new_run(1);
    assert_eq!(
        game.players[0].boons,
        Boons::default(),
        "a fresh run should start with nothing"
    );
}

#[test]
fn a_boon_already_held_is_never_offered_again() {
    let v = Viewport::new(1280.0, 800.0);
    let mut rng = Rng::new(31337);
    let held = Boons { double_jump: true, shield: true, ..Default::default() };
    for _ in 0..500 {
        let offer = Offer::roll(&v, 0.0, 700.0, AttackKind::Basic, 1, held, 0, &mut rng);
        for choice in offer.choices.iter() {
            if let OfferItem::Boon(boon) = choice.item {
                assert!(!held.has(boon), "{boon:?} was offered though it is held");
            }
        }
    }
}

#[test]
fn a_drained_pool_has_only_the_wall_swap_left() {
    // What stops boons swallowing the weapon roster: the pool drains. It never
    // empties completely, because the two walls are one setting and the one not
    // in use is always a live choice - but one option out of eleven is not what
    // swallows a roster.
    let v = Viewport::new(1280.0, 800.0);
    let mut rng = Rng::new(4242);
    let all = Boons {
        double_jump: true,
        dash_free: true,
        shield: true,
        wall: WallMod::Push,
    };
    let mut weapons = 0;
    let mut boons = 0;
    for _ in 0..200 {
        let offer = Offer::roll(&v, 0.0, 700.0, AttackKind::Hammer, 2, all, 0, &mut rng);
        for choice in offer.choices.iter() {
            match choice.item {
                OfferItem::Attack { .. } => weapons += 1,
                OfferItem::Boon(b) => {
                    assert_eq!(b, Boon::WallPull, "a spent boon came back round");
                    boons += 1;
                }
            }
        }
    }
    assert!(weapons > boons * 3, "a drained pool should be mostly weapons");
}

#[test]
fn every_boon_can_come_up() {
    let v = Viewport::new(1280.0, 800.0);
    let mut rng = Rng::new(777);
    let mut seen = [false; Boon::ALL.len()];
    let mut saw_weapon = false;
    for _ in 0..2000 {
        let offer = Offer::roll(
            &v,
            0.0,
            700.0,
            AttackKind::Basic,
            1,
            Boons::default(),
            0,
            &mut rng,
        );
        for choice in offer.choices.iter() {
            match choice.item {
                OfferItem::Boon(b) => {
                    seen[Boon::ALL.iter().position(|x| *x == b).unwrap()] = true
                }
                OfferItem::Attack { .. } => saw_weapon = true,
            }
        }
    }
    assert!(seen.iter().all(|s| *s), "some boon can never be drawn");
    assert!(saw_weapon, "weapons should still share the pool");
}

/* ---------------- the modified wall ---------------- */

/// A run with `wall`, one enemy planted `offset` from the player, and a charge
/// ready to spend.
fn wall_and_one_enemy(wall: WallMod, offset: f32) -> Game {
    let mut game = player_with(Boons { wall, ..Default::default() });
    let v = game.viewport;
    game.state = State::Playing;
    game.players[0].super_charges = 1;
    let mut z = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    z.body.x = game.players[0].body.x + offset;
    z.body.y = v.hper(GROUND_Y_PCT) - z.body.h;
    z.ax = 0.0;
    game.zombies.clear();
    game.flyers.clear();
    game.zombies.push(z);
    game
}

/// One tick of jumping, which is what the slam needs to be airborne.
fn leave_the_floor(game: &mut Game) {
    let mut frame = idle();
    frame.players[0].jump = true;
    game.tick(&frame);
}

/// One tick of the slam button, which is what raises the wall.
///
/// Note this tick also runs the enemies, so the first step of the shove is
/// spent inside it - a measurement taken afterwards has already missed a fifth
/// of the distance.
fn slam(game: &mut Game) {
    let mut frame = idle();
    frame.players[0].slam = true;
    game.tick(&frame);
}

/// Raises the wall the way the game does - in the air, with the slam button.
fn slam_up_the_wall(game: &mut Game) {
    leave_the_floor(game);
    slam(game);
}

/// How far the one enemy on the field is moved over `ticks`, counting from the
/// tick the wall goes up.
///
/// The shove is written into the enemy's own velocity, so there is no moment it
/// stops: it bleeds off. What can be pinned down is the distance over a window,
/// which is what [`WALL_SHOVE_AX_PCT`] is aimed at.
fn distance_moved_by_the_wall(game: &mut Game, flyer: bool, ticks: i32) -> f32 {
    let read = |g: &Game| if flyer { g.flyers[0].body.x } else { g.zombies[0].body.x };
    leave_the_floor(game);
    let before = read(game);
    slam(game);
    for _ in 1..ticks {
        game.tick(&idle());
    }
    read(game) - before
}

#[test]
fn a_plain_wall_leaves_the_field_where_it_stands() {
    // Not "ax is zero": an enemy walking toward the player has a speed of its
    // own, and it was never the wall that gave it one.
    let mut game = wall_and_one_enemy(WallMod::Plain, 400.0);
    let want = game.viewport.wper(10.0);
    let travelled = distance_moved_by_the_wall(&mut game, false, WALL_SHOVE_TICKS).abs();
    assert!(
        travelled < want / 4.0,
        "an unmodified wall moved the field {travelled}"
    );
}

#[test]
fn a_black_wall_drags_the_field_in() {
    let mut game = wall_and_one_enemy(WallMod::Pull, 400.0);
    let want = game.viewport.wper(10.0);
    let travelled = -distance_moved_by_the_wall(&mut game, false, WALL_SHOVE_TICKS);
    assert!(
        (travelled - want).abs() < want * 0.02,
        "it should have come {want} closer, not {travelled}"
    );
}

#[test]
fn a_grey_wall_clears_the_field_out() {
    let mut game = wall_and_one_enemy(WallMod::Push, 400.0);
    let want = game.viewport.wper(10.0);
    let travelled = distance_moved_by_the_wall(&mut game, false, WALL_SHOVE_TICKS);
    assert!(
        (travelled - want).abs() < want * 0.02,
        "it should have been thrown {want} clear, not {travelled}"
    );
}

#[test]
fn the_field_is_moved_toward_or_away_whichever_side_it_is_on() {
    // Read off the side the enemy stands on, not off which way it happens to
    // be walking - otherwise a wall would gather half the field and scatter
    // the rest.
    for offset in [400.0, -400.0] {
        let mut game = wall_and_one_enemy(WallMod::Pull, offset);
        let gap = |g: &Game| (g.zombies[0].body.x - g.players[0].body.x).abs();
        let before = gap(&game);
        slam_up_the_wall(&mut game);
        for _ in 0..WALL_SHOVE_TICKS {
            game.tick(&idle());
        }
        assert!(gap(&game) < before, "an enemy at {offset} was not drawn in");
    }
}

#[test]
fn the_hold_is_one_shove_and_not_a_standing_force() {
    // A lasting pull is a tractor beam and a lasting push makes the wall
    // unapproachable. Neither is a wall.
    let mut game = wall_and_one_enemy(WallMod::Push, 400.0);
    slam_up_the_wall(&mut game);
    for _ in 0..60 {
        game.tick(&idle());
    }
    let after = game.zombies[0].body.x;
    for _ in 0..20 {
        game.tick(&idle());
    }
    assert!(
        game.zombies[0].body.x < after,
        "it should have gone back to walking in"
    );
}

#[test]
fn a_leaper_mid_leap_is_moved_like_everything_else() {
    // Its trajectory takes an early exit out of the chase; the shove has to be
    // checked before that or the one enemy hardest to catch would ignore it.
    let mut game = wall_and_one_enemy(WallMod::Push, 300.0);
    let v = game.viewport;
    game.zombies[0].movement = Movement::Leap(Leap { crouch: 0, airborne: true });
    game.zombies[0].ax = -v.wper(2.0);
    game.zombies[0].body.y -= v.hper(20.0);
    let before = game.zombies[0].body.x;

    slam_up_the_wall(&mut game);
    game.tick(&idle());
    assert!(
        game.zombies[0].body.x > before,
        "a leaper flew through the wall's hold untouched"
    );
}

#[test]
fn flyers_are_field_too() {
    let mut game = player_with(Boons { wall: WallMod::Push, ..Default::default() });
    let v = game.viewport;
    game.state = State::Playing;
    game.players[0].super_charges = 1;
    game.zombies.clear();
    game.flyers.clear();
    let size = game.players[0].body;
    let mut f = Flyer::from_edge(&v, &size, 0, &mut game.rng);
    f.body.x = game.players[0].body.x + 300.0;
    game.flyers.push(f);
    let want = v.wper(10.0);
    let travelled = distance_moved_by_the_wall(&mut game, true, WALL_SHOVE_TICKS);
    assert!(
        (travelled - want).abs() < want * 0.02,
        "a flyer should be thrown {want} clear, not {travelled}"
    );
}

#[test]
fn the_shove_coasts_rather_than_stopping_dead() {
    // Living in the enemy's own velocity means there is no tick it ends on. The
    // window the constant is aimed at is the first five; after that it keeps
    // going, more slowly, for about another fifteen.
    let mut game = wall_and_one_enemy(WallMod::Push, 400.0);
    let v = game.viewport;
    let five = distance_moved_by_the_wall(&mut game, false, WALL_SHOVE_TICKS);
    let mut game = wall_and_one_enemy(WallMod::Push, 400.0);
    let twenty = distance_moved_by_the_wall(&mut game, false, 20);
    assert!(
        twenty > five * 2.0,
        "it should still be travelling after the window: {five} then {twenty}"
    );
    assert!(
        twenty < v.wper(30.0),
        "but a third of the view would be a launch, not a shove"
    );
}

#[test]
fn a_shoved_flyer_goes_back_to_cruising() {
    // Nothing in a flyer's own update ever lowers its speed, so without the
    // bleed-off the shove would become the speed it keeps for good.
    let mut game = player_with(Boons { wall: WallMod::Push, ..Default::default() });
    let v = game.viewport;
    game.state = State::Playing;
    game.players[0].super_charges = 1;
    game.zombies.clear();
    game.flyers.clear();
    let size = game.players[0].body;
    let mut f = Flyer::from_edge(&v, &size, 0, &mut game.rng);
    f.body.x = game.players[0].body.x + 300.0;
    game.flyers.push(f);

    slam_up_the_wall(&mut game);
    assert!(game.flyers[0].ax.abs() > v.wper(FLYER_CRUISE_PCT));
    for _ in 0..60 {
        game.tick(&idle());
    }
    assert!(
        game.flyers[0].ax.abs() <= v.wper(FLYER_CRUISE_PCT) + 0.01,
        "a shoved flyer kept the speed for good: {}",
        game.flyers[0].ax
    );
}

#[test]
fn nothing_moving_at_its_own_pace_is_touched_by_the_bleed_off() {
    // The deceleration only ever applies above cruising speed, so an ordinary
    // flyer crosses the screen exactly as it always did.
    let mut game = new_game();
    let v = game.viewport;
    game.state = State::Playing;
    game.zombies.clear();
    game.flyers.clear();
    let size = Body::new(0.0, 0.0, v.wper(5.0), v.hper(10.0));
    game.flyers.push(Flyer::from_edge(&v, &size, 0, &mut game.rng));
    let cruise = game.flyers[0].ax;
    for _ in 0..30 {
        game.tick(&idle());
    }
    assert_eq!(game.flyers[0].ax, cruise, "cruising speed should be left alone");
}

#[test]
fn the_two_walls_replace_each_other() {
    // A wall cannot pull and push at once, so they are one setting. That also
    // means the offer keeps showing the other one, which is the swap.
    let mut boons = Boons::default();
    boons.take(Boon::WallPull);
    assert_eq!(boons.wall, WallMod::Pull);
    assert!(boons.has(Boon::WallPull) && !boons.has(Boon::WallPush));

    boons.take(Boon::WallPush);
    assert_eq!(boons.wall, WallMod::Push);
    assert!(boons.has(Boon::WallPush) && !boons.has(Boon::WallPull));
}

#[test]
fn holding_one_wall_leaves_the_other_on_the_table() {
    let v = Viewport::new(1280.0, 800.0);
    let mut rng = Rng::new(9001);
    let held = Boons {
        double_jump: true,
        dash_free: true,
        shield: true,
        wall: WallMod::Pull,
    };
    let mut saw_push = false;
    for _ in 0..500 {
        let offer = Offer::roll(&v, 0.0, 700.0, AttackKind::Basic, 1, held, 0, &mut rng);
        for choice in offer.choices.iter() {
            if let OfferItem::Boon(b) = choice.item {
                assert_ne!(b, Boon::WallPull, "the wall it already has was offered");
                saw_push = true;
            }
        }
    }
    assert!(saw_push, "the other wall should still come up as a swap");
}

#[test]
fn each_wall_is_a_flat_slab_of_its_own_shade() {
    // Three shades far enough apart to be told apart at a glance, and nothing
    // else: no outline, so the colour carries the whole warning.
    let shades = [WallMod::Plain, WallMod::Pull, WallMod::Push].map(|w| w.color().r);
    assert!(shades[0] > 200.0, "the plain wall should be white");
    assert!(shades[1] < 32.0, "the pulling wall should be black");
    assert!(shades[2] > 100.0 && shades[2] < 200.0, "the pushing wall should be grey");
    for pair in [(0, 1), (1, 2), (0, 2)] {
        let gap = (shades[pair.0] - shades[pair.1]).abs();
        assert!(gap > 90.0, "two walls are only {gap} apart");
    }
}

/* ---------------- the offer between waves ---------------- */

/// Ticks until the standing options accept a hit.
fn wait_for_armed(game: &mut Game) {
    for _ in 0..OFFER_ARM_TICKS + 4 {
        if game.offer.as_ref().is_some_and(|o| o.armed(game.timer)) {
            return;
        }
        game.zombies.clear();
        game.flyers.clear();
        game.tick(&idle());
    }
    panic!("the offer never went live");
}

/// Runs a wave out so the lull - and any offer with it - begins.
fn into_the_lull(game: &mut Game) {
    for _ in 0..8000 {
        game.zombies.clear();
        game.flyers.clear();
        game.tick(&idle());
        if game.waves.between_waves() {
            return;
        }
    }
    panic!("the wave never cleared");
}

#[test]
fn an_offer_arrives_once_the_team_has_earned_it() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    assert!(game.offer.is_none(), "nothing is owed at the start");

    game.players[0].score = OFFER_SCORE_STEP;
    into_the_lull(&mut game);
    let offer = game.offer.as_ref().expect("the lull should be offering");
    assert_eq!(offer.choices.len(), 3);
}

#[test]
fn the_three_options_are_all_different() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].score = OFFER_SCORE_STEP;
    into_the_lull(&mut game);

    let c = &game.offer.as_ref().unwrap().choices;
    assert!(c[0].item != c[1].item && c[1].item != c[2].item && c[0].item != c[2].item);
    for choice in c.iter() {
        assert_ne!(
            choice.item,
            OfferItem::Attack { kind: AttackKind::Basic, level: 1 },
            "the plain attack is not an upgrade"
        );
    }
}

#[test]
fn an_offer_ignores_the_swing_that_ended_the_wave() {
    // The wave ends while the player is still swinging at the last of it, and
    // the options appear inside that swing. Found in play: every option was
    // being taken by accident.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].score = OFFER_SCORE_STEP;
    into_the_lull(&mut game);

    let offer = game.offer.as_ref().expect("the lull should be offering");
    assert!(!offer.armed(game.timer), "it must not be live the moment it lands");
    let target = offer.choices[0].body;

    // Swinging at one for the whole delay changes nothing.
    let mut swings = 0;
    while !game.offer.as_ref().is_some_and(|o| o.armed(game.timer)) {
        game.zombies.clear();
        game.flyers.clear();
        swing_at(&mut game, 0, target);
        game.tick(&idle());
        swings += 1;
        assert!(
            game.offer.is_some(),
            "an option was taken after {swings} swings, before the offer went live"
        );
    }
    assert!(swings > 60, "the pause was only {swings} ticks - too short to help");
    assert_eq!(game.players[0].attack, AttackKind::Basic, "nothing was taken");

    // And once it is live, the same swing takes it.
    assert!(game.offer.as_ref().unwrap().armed(game.timer));
    swing_at(&mut game, 0, target);
    game.tick(&idle());
    assert!(game.offer.is_none(), "it should be takeable now");
}

#[test]
fn hitting_an_option_arms_every_player() {
    // One offer, shared: either player may take it and both end up carrying it.
    let mut game = new_game();
    game.start_run(2);
    settle_on_floor(&mut game);
    game.players[0].score = OFFER_SCORE_STEP;
    into_the_lull(&mut game);

    let taken = game.offer.as_ref().unwrap().choices[1];
    wait_for_armed(&mut game);
    swing_at(&mut game, 0, taken.body);
    game.tick(&idle());

    assert!(game.offer.is_none(), "taking one clears the rest");
    for p in game.players.iter() {
        match taken.item {
            OfferItem::Attack { kind, level } => {
                assert_eq!(p.attack, kind, "both players should be carrying it");
                assert_eq!(p.attack_level, level);
            }
            OfferItem::Boon(boon) => {
                assert!(p.boons.has(boon), "both players should have been given it");
            }
        }
    }
}

#[test]
fn an_offer_can_be_declined_by_starting_the_wave() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].score = OFFER_SCORE_STEP;
    into_the_lull(&mut game);
    assert!(game.offer.is_some());

    game.waves.skip_countdown();
    game.tick(&idle());
    assert!(game.offer.is_none(), "the lull ending takes it off the table");
    assert_eq!(game.players[0].attack, AttackKind::Basic, "and nothing was taken");
}

#[test]
fn the_view_is_pinned_while_the_choice_is_up() {
    // The options stand in the view; walking off would lose them.
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].score = OFFER_SCORE_STEP;
    into_the_lull(&mut game);

    let camera = game.camera_x;
    let mut walk = idle();
    walk.players[0].right = true;
    for _ in 0..120 {
        game.zombies.clear();
        game.flyers.clear();
        game.tick(&walk);
    }
    assert_eq!(game.camera_x, camera, "the view should not have moved");
}

#[test]
fn a_second_offer_costs_another_full_step() {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    game.players[0].score = OFFER_SCORE_STEP;
    into_the_lull(&mut game);

    let first = game.offer.as_ref().unwrap().choices[0];
    wait_for_armed(&mut game);
    swing_at(&mut game, 0, first.body);
    game.tick(&idle());
    assert!(game.offer.is_none());

    // Same score, next lull: nothing more is owed.
    game.waves.skip_countdown();
    into_the_lull(&mut game);
    assert!(game.offer.is_none(), "one score should not buy two");

    game.players[0].score = OFFER_SCORE_STEP * 2;
    game.waves.skip_countdown();
    into_the_lull(&mut game);
    assert!(game.offer.is_some(), "another step should buy another");
}

#[test]
fn the_options_are_rolled_from_their_own_generator() {
    // Which three turn up must not shift which waves and variants follow, the
    // same reason the soundtrack has a generator of its own.
    let wave_stream = |offered: bool| {
        let mut game = new_game();
        game.start_run(1);
        settle_on_floor(&mut game);
        if offered {
            game.players[0].score = OFFER_SCORE_STEP;
        }
        into_the_lull(&mut game);
        (game.waves.kind, game.waves.rule)
    };
    assert_eq!(
        wave_stream(false),
        wave_stream(true),
        "rolling an offer changed the wave that followed"
    );
}

/* ---------------- the combo finisher ---------------- */

/// Kills one enemy with a swing and reports the blasts left behind.
///
/// The bystander test below does not go through a swing at all: a full combo
/// doubles the attack box to 13% of the view while the blast only reaches 10%
/// out from where it started, so there is no spot that is inside one and
/// outside the other. The two have to be checked apart.
fn blast_damage_after_kill(combo: u32) -> f32 {
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    clear_arena(&mut game);

    let v = game.viewport;
    let mut victim = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    victim.body.y = v.hper(GROUND_Y_PCT) - victim.body.h;
    victim.body.x = game.players[0].body.x + v.wper(6.0);
    victim.hp = 1.0;
    let victim_body = victim.body;
    game.zombies.push(victim);
    game.flyers.clear();

    game.players[0].combo = combo;
    swing_at(&mut game, 0, victim_body);
    // The death check runs before the damage does, so the kill - and the blast
    // with it - lands on the tick after the one that took the last of its
    // health.
    for _ in 0..3 {
        game.flyers.clear();
        game.tick(&idle());
    }

    game.explosions
        .iter()
        .map(|e| e.damage)
        .fold(0.0f32, f32::max)
}

#[test]
fn a_combo_kill_leaves_a_blast_that_bites() {
    // What keeping the rhythm is worth - and what every upgrade trades away,
    // since only the plain attack makes one.
    assert_eq!(blast_damage_after_kill(2), COMBO_BLAST_DAMAGE);
}

#[test]
fn an_ordinary_kill_leaves_a_blast_that_does_not() {
    assert_eq!(blast_damage_after_kill(0), 0.0, "a plain death left a live blast");
}

#[test]
fn a_live_blast_bites_once_however_long_it_covers_you() {
    // It grows over a hundred ticks; damage on each of them would make the
    // finisher worth more than a wall. Placed by hand rather than earned, so
    // the swing that would have made it cannot also reach the target.
    const MARK: f32 = 777_777.0;
    let mut game = new_game();
    game.start_run(1);
    settle_on_floor(&mut game);
    clear_arena(&mut game);

    let v = game.viewport;
    let mut target = Zombie::from_edge(&v, &mut game.rng, TEST_ENEMY_COLOR);
    target.body.y = v.hper(GROUND_Y_PCT) - target.body.h;
    target.body.x = game.players[0].body.x + v.wper(40.0);
    target.hp = MARK;
    target.hpmax = MARK;
    target.speed_multiplier = 0.0;
    let at = target.body;
    game.zombies.push(target);
    game.flyers.clear();

    game.explosions.push(Explosion::lethal(
        at.x,
        at.y,
        &v,
        COMBO_BLAST_DAMAGE,
        4242,
    ));

    for _ in 0..120 {
        game.flyers.clear();
        game.tick(&idle());
    }
    let left = game
        .zombies
        .iter()
        .find(|z| z.hpmax == MARK)
        .expect("the target wandered off")
        .hp;
    assert_eq!(MARK - left, COMBO_BLAST_DAMAGE, "exactly one bite");
}

#[test]
fn the_generator_is_usable_in_its_low_bits() {
    // Everything that picks between a handful of things takes a remainder, and
    // a remainder reads the lowest bits. A raw xorshift's lowest bits follow a
    // recurrence short enough to see: `range(0, 3)` once went four hundred
    // draws in a live run without ever returning 0, which took one wave rule
    // out of five out of the game.
    let mut rng = Rng::new(1);
    let mut counts = [0usize; 4];
    const DRAWS: usize = 40_000;
    for _ in 0..DRAWS {
        counts[rng.range(0, 3) as usize] += 1;
    }
    let want = DRAWS / 4;
    for (value, seen) in counts.iter().enumerate() {
        let off = (*seen as i64 - want as i64).abs();
        assert!(
            off < want as i64 / 10,
            "range(0, 3) returned {value} {seen} times in {DRAWS}, wanted about {want}"
        );
    }

    // The single lowest bit, which is what `flip` reads.
    let mut rng = Rng::new(1);
    let heads = (0..DRAWS).filter(|_| rng.flip()).count();
    let off = (heads as i64 - want as i64 * 2).abs();
    assert!(off < DRAWS as i64 / 20, "flip came up heads {heads} times in {DRAWS}");
}

#[test]
fn every_wave_rule_still_reaches_a_real_run() {
    // The distribution above is the mechanism; this is the thing that broke.
    let mut game = new_game();
    game.start_run(1);
    let mut seen = alloc::vec![false; 5];
    for _ in 0..400 {
        let was = game.wave;
        for _ in 0..200 {
            game.spawn_count = wave_budget(game.wave);
            game.zombies.clear();
            game.flyers.clear();
            game.tick(&idle());
            if game.wave > was {
                break;
            }
        }
        let i = match game.waves.rule {
            WaveRule::Normal => 0,
            WaveRule::StaticCamera => 1,
            WaveRule::NoJumps => 2,
            WaveRule::NoWall => 3,
            WaveRule::Hidden => 4,
        };
        seen[i] = true;
    }
    assert!(seen.iter().all(|s| *s), "some rule never came up: {seen:?}");
}

/* ---------------- the rolled boss ---------------- */

/// Runs a boss wave and reports what it put out.
fn boss_wave(wave: i64, seed: u64) -> (alloc::vec::Vec<BossKind>, usize, bool) {
    let mut manager = WaveManager::default();
    let mut rng = Rng::new(seed);
    manager.begin_wave(wave, &mut rng);
    manager.skip_countdown();
    let (mut spawned, mut plain, mut cleared) = (0i64, 0usize, false);
    let mut kinds = alloc::vec::Vec::new();
    for _ in 0..500_000 {
        match manager.update(wave, spawned, 0, &mut rng) {
            WaveAction::Idle => {}
            WaveAction::ClearWave => {
                cleared = true;
                break;
            }
            WaveAction::SpawnBosses(n) => {
                for _ in 0..n {
                    kinds.push(BossKind::Ground);
                }
                spawned += n;
            }
            WaveAction::SpawnFlyingBoss => {
                kinds.push(BossKind::Flying);
                spawned += 1;
            }
            WaveAction::SpawnShedderBoss => {
                kinds.push(BossKind::Shedder);
                spawned += 1;
            }
            WaveAction::SpawnRolledBoss => {
                kinds.push(BossKind::Rolled);
                spawned += 1;
            }
            WaveAction::SpawnBossGroup(k) => {
                spawned += k.len() as i64;
                kinds.extend(k);
            }
            WaveAction::SpawnElite(n) => spawned += n as i64,
            _ => {
                plain += 1;
                spawned += 1;
            }
        }
    }
    (kinds, plain, cleared)
}

#[test]
fn the_twentieth_wave_is_one_boss_and_nothing_else() {
    let (kinds, plain, cleared) = boss_wave(ROLLED_BOSS_WAVE, 4242);
    assert_eq!(kinds, alloc::vec![BossKind::Rolled]);
    assert_eq!(plain, 0, "the wave should spawn nothing but the boss");
    assert!(cleared, "and it still has to be able to end");
}

#[test]
fn a_wave_that_spawns_nothing_can_still_finish() {
    // The trap in switching ordinary spawning off: a wave ends when its budget
    // is spent, and a wave that never spawns never spends one. It would sit a
    // hundred and fifty short of a number it was never going to reach.
    let (_, _, cleared) = boss_wave(ROLLED_BOSS_WAVE, 7);
    assert!(cleared);
    // And the HUD must not count against that number either.
    let mut game = new_game();
    game.start_run(1);
    game.wave = ROLLED_BOSS_WAVE;
    assert_eq!(game.wave_kill_target(), None);
    game.wave = ROLLED_BOSS_WAVE + 1;
    assert_eq!(game.wave_kill_target(), Some(wave_budget(ROLLED_BOSS_WAVE + 1)));
}

#[test]
fn the_rolled_boss_is_a_boss_by_every_measure_the_game_uses() {
    let mut game = new_game();
    let v = game.viewport;
    let recipe = Recipe {
        movement: MoveKind::Run,
        size: Size::Normal,
        shoot: false,
        blink: false,
        shed: true,
        brood: false,
    };
    let boss = recipe.build_boss(&v, ROLLED_BOSS_WAVE, 0, &mut game.rng);
    assert!(boss.is_boss, "the wall and the payout both read this flag");
    assert_eq!(boss.hpmax, boss_hp(ROLLED_BOSS_WAVE));
    assert_eq!(boss.hpmax, Zombie::boss(&v, ROLLED_BOSS_WAVE, &mut game.rng).hpmax);
    assert_eq!(boss.armor, 1.0, "boss health is the difficulty; armour on top is not");

    // Two traits are forced whatever the roll said.
    assert!(boss.broods, "making its own crowd is the whole fight");
    assert!(
        !boss.behaviors.shed,
        "a hazard left by something that takes hundreds of hits would fill the floor"
    );
}

#[test]
fn late_boss_waves_grow_and_mix() {
    for (wave, want) in [(25i64, 2usize), (30, 3), (35, 4), (40, 5)] {
        let (kinds, _, _) = boss_wave(wave, wave as u64 * 13 + 1);
        assert_eq!(kinds.len(), want, "wave {wave} carried the wrong number");
    }
    // Drawn separately, so a late wave is a mixed set rather than a row of the
    // same thing. Over enough runs every kind has to turn up.
    let mut seen = alloc::vec![false; BossKind::ALL.len()];
    for seed in 0..200u64 {
        for kind in boss_wave(40, seed).0 {
            seen[BossKind::ALL.iter().position(|k| *k == kind).unwrap()] = true;
        }
    }
    assert!(seen.iter().all(|s| *s), "some boss kind is never drawn: {seen:?}");
}

#[test]
fn the_early_boss_waves_are_left_as_they_were() {
    assert_eq!(boss_wave(5, 1).0, alloc::vec![BossKind::Ground]);
    assert_eq!(boss_wave(FLYING_BOSS_WAVE, 1).0, alloc::vec![BossKind::Flying]);
    // Fifteen is a coin flip between three ground bosses and one shedder.
    let mut saw_both = (false, false);
    for seed in 0..40u64 {
        match boss_wave(SHEDDER_BOSS_WAVE, seed).0.len() {
            1 => saw_both.0 = true,
            3 => saw_both.1 = true,
            n => panic!("wave 15 produced {n} bosses"),
        }
    }
    assert!(saw_both.0 && saw_both.1, "the coin flip stopped being a coin flip");
}

#[test]
fn a_rolled_boss_is_announced_as_what_it_actually_is() {
    // The announcement is the only warning the player gets, and two of the
    // traits are decided rather than drawn. Naming the roll would promise a
    // hazard the boss does not leave and hide the brood it always has.
    let mut rng = Rng::new(99);
    for _ in 0..200 {
        let rolled = Recipe::roll(ROLLED_BOSS_WAVE, &mut rng);
        let named = rolled.as_boss();
        assert!(named.brood, "every one of them broods");
        assert!(!named.shed, "and none of them sheds");
        let label = named.label();
        assert!(label.contains("BROOD"), "the name hides the brood: {label}");
        assert!(!label.contains("TRAP"), "the name promises a hazard: {label}");
    }
}

/* ---------------- the bands ---------------- */

#[test]
fn the_bands_and_the_name_say_the_same_thing() {
    // The stripes are the announcement drawn instead of written, and they go on
    // saying it after the name has faded. A trait in one and not the other
    // would make them two different claims about the same enemy.
    let mut rng = Rng::new(20260822);
    let mut marks = [Rgb::new(0.0, 0.0, 0.0); MAX_MARKS];
    for _ in 0..500 {
        let recipe = Recipe::roll(40, &mut rng);
        let words = recipe.label().split_whitespace().count();
        let bands = recipe.marks(&mut marks);
        assert_eq!(bands, words, "{} reads as {words} and draws as {bands}", recipe.label());
        assert!(bands >= 1 && bands <= MAX_MARKS);
    }
}

#[test]
fn the_band_order_follows_the_name() {
    // Size, then how it moves, then what it does - the order the name reads in.
    let recipe = Recipe {
        size: Size::Large,
        movement: MoveKind::Leap,
        shoot: true,
        blink: false,
        shed: false,
        brood: true,
    };
    let mut marks = [Rgb::new(0.0, 0.0, 0.0); MAX_MARKS];
    let n = recipe.marks(&mut marks);
    assert_eq!(recipe.label(), "HUGE LEAPER GUN BROOD");
    assert_eq!(n, 4);
    assert_eq!(
        &marks[..4],
        &[recipe_marks::HUGE, recipe_marks::LEAPER, recipe_marks::GUN, recipe_marks::BROOD]
    );

    // The ordinary size has no word and no band, the same absence twice.
    let plain = Recipe { size: Size::Normal, ..recipe };
    assert_eq!(plain.marks(&mut marks), 3);
    assert_eq!(marks[0], recipe_marks::LEAPER);
}

#[test]
fn no_two_marks_can_be_mistaken_for_each_other_or_for_the_ground() {
    // Eyeballed sets kept putting a blue next to the lull sky. This is the
    // measurement that stopped that, kept so a future edit has to face it too.
    let marks = [
        recipe_marks::HUGE,
        recipe_marks::SMALL,
        recipe_marks::SLIGHT,
        recipe_marks::RUNNER,
        recipe_marks::HOPPER,
        recipe_marks::LEAPER,
        recipe_marks::FLIER,
        recipe_marks::GUN,
        recipe_marks::BLINK,
        recipe_marks::TRAP,
        recipe_marks::BROOD,
    ];
    // Weighted so green counts for more than blue, roughly as an eye does.
    let apart = |a: Rgb, b: Rgb| {
        let rm = (a.r + b.r) / 2.0;
        let (dr, dg, db) = (a.r - b.r, a.g - b.g, a.b - b.b);
        libm::sqrtf((2.0 + rm / 256.0) * dr * dr + 4.0 * dg * dg
            + (2.0 + (255.0 - rm) / 256.0) * db * db)
    };
    for (i, a) in marks.iter().enumerate() {
        for b in marks.iter().skip(i + 1) {
            assert!(apart(*a, *b) > 150.0, "{a:?} and {b:?} are too close");
        }
        // Against the body it is drawn on and everything behind it.
        for ground in [
            ELITE_COLOR,
            Rgb::new(240.0, 50.0, 20.0),
            Rgb::new(80.0, 170.0, 255.0),
            Rgb::new(80.0, 255.0, 130.0),
            Rgb::new(0.0, 0.0, 0.0),
        ] {
            assert!(apart(*a, ground) > 150.0, "{a:?} vanishes on {ground:?}");
        }
    }
}

#[test]
fn only_the_heavies_are_banded() {
    // Young wear the same shade as the parent and are left flat. That is the
    // difference the player needs at a glance: the striped one is the one worth
    // the swings.
    let mut game = new_game();
    let v = game.viewport;
    let recipe = Recipe {
        movement: MoveKind::Run,
        size: Size::Normal,
        shoot: false,
        blink: false,
        shed: false,
        brood: true,
    };
    let heavy = recipe.build(&v, 20, 0, &mut game.rng);
    let minion = recipe.build_minion(&v, 0, &mut game.rng);
    assert!(heavy.elite && heavy.recipe.is_some(), "a heavy carries what it was rolled from");
    assert!(!minion.elite && !minion.is_boss, "and a minion is neither");
    assert_eq!(heavy.color, minion.color, "but they share a shade");
}
