//! Simulation tests. These drive `Game` headlessly (no window, no rendering),
//! which is how the JS original was verified too.

use alloc::vec;
use alloc::vec::Vec;

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
    game.players[player].attack_ticks = ATTACK_TICKS;
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
    shooter.behavior = Behavior::Shooter { cooldown: 0 };
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
    jumper.behavior = Behavior::Jumper { cooldown: 0 };
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
            WaveAction::SpawnBosses(n) => return Some(WaveAction::SpawnBosses(n)),
            WaveAction::SpawnFlyingBoss => return Some(WaveAction::SpawnFlyingBoss),
            WaveAction::SpawnShedderBoss => return Some(WaveAction::SpawnShedderBoss),
            _ => {}
        }
    }
    None
}

#[test]
fn wave_ten_belongs_to_the_flying_boss_and_the_others_keep_ground_bosses() {
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
    // Past it, the ordinary boss waves carry on unchanged.
    assert_eq!(boss_action_for(20), Some(WaveAction::SpawnBosses(4)));
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
    z.behavior = Behavior::Shooter { cooldown: SHOOTER_AIM_TICKS + 4 };
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
    z.behavior = Behavior::Shooter { cooldown: SHOOTER_AIM_TICKS };
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
    for _ in 0..400 {
        game.spawn_count = game.wave * 10;
        game.zombies.clear();
        game.flyers.clear();
        let was = game.wave;
        for _ in 0..80 {
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
    panic!("never rolled a held wave");
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

    let mut spawned_at = |wave: i64, live: usize, game: &mut Game| {
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

/* ---------------- the wall against bosses ---------------- */

/// Puts an active ultimate field over `body`, as if the player had slammed.
fn raise_wall_over(game: &mut Game, body: Body) {
    game.players[0].field.active = true;
    game.players[0].field.body = body;
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
fn a_husk_still_hurts_a_dashing_player() {
    // The husk is the one thing a dash cannot shove: it takes no damage and
    // does not move, which makes it the counter to the crowd-clear.
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
        .find(|z| z.behavior == Behavior::Shedder)
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
        .position(|z| z.behavior == Behavior::Shedder)
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
        .position(|z| z.behavior == Behavior::Shedder)
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
        .position(|z| z.behavior == Behavior::Shedder)
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
        !game.zombies.iter().any(|z| z.behavior == Behavior::Shedder),
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
        .position(|z| z.behavior == Behavior::Shedder)
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
    assert_eq!(boss.behavior, Behavior::Shedder);
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
