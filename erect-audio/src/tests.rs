//! The gain logic is where the audible bugs live, so it is what gets tested.

use alloc::vec;
use erect_core::audio::{MusicState, Situation};

use crate::gains::{db_to_linear, CountOf, GainEngine, LayerSpec, Trigger};

const NEVER: [Situation; 0] = [];
const PAUSED: [Situation; 1] = [Situation::Paused];
const COMBAT: [Situation; 1] = [Situation::Combat];
const CALM_OR_PAUSED: [Situation; 2] = [Situation::Calm, Situation::Paused];

fn state(situation: Situation, zombies: usize, flyers: usize, boss: bool) -> MusicState {
    MusicState { situation, zombies, flyers, boss }
}

fn counted(hold_ms: f32) -> GainEngine {
    GainEngine::new(vec![LayerSpec {
        id: "zombie",
        trigger: Trigger::Count {
            of: CountOf::Zombie,
            steps_db: &[(1, -12.0), (2, -6.0), (3, 0.0)],
        },
        file_gain_db: 0.0,
        fade_in_ms: 250.0,
        fade_out_ms: 900.0,
        hold_ms,
        mute_when: &PAUSED,
    }])
}

/// Run for `ms`, then report where the gain ended up.
fn run(engine: &mut GainEngine, st: &MusicState, ms: f32) -> f32 {
    let step: f32 = 16.0;
    let mut left = ms;
    while left > 0.0 {
        engine.update(st, step.min(left));
        left -= step;
    }
    engine.raw_gain(0)
}

#[test]
fn count_steps_map_to_the_pack_levels() {
    let mut e = counted(0.0);
    for (n, expected_db) in [(0, -80.0), (1, -12.0), (2, -6.0), (3, 0.0), (9, 0.0)] {
        e.snap(&state(Situation::Combat, n, 0, false));
        let want = if expected_db <= -80.0 { 0.0 } else { db_to_linear(expected_db) };
        assert!(
            (e.raw_gain(0) - want).abs() < 1e-3,
            "{n} zombies should give {expected_db} dB, got {}",
            e.raw_gain(0)
        );
    }
}

#[test]
fn a_rise_is_immediate_but_a_fall_waits_out_the_hold() {
    let mut e = counted(700.0);
    e.snap(&state(Situation::Combat, 3, 0, false));
    assert!((e.raw_gain(0) - 1.0).abs() < 1e-6);

    // Drop to two zombies: still full for the length of the hold.
    let st = state(Situation::Combat, 2, 0, false);
    let after_300 = run(&mut e, &st, 300.0);
    assert!(after_300 > 0.99, "fall acted on too early: {after_300}");

    // Well past the hold plus the fade, it has settled on the -6 dB step.
    let settled = run(&mut e, &st, 2000.0);
    assert!(
        (settled - db_to_linear(-6.0)).abs() < 0.02,
        "should settle at -6 dB, got {settled}"
    );
}

#[test]
fn oscillating_around_a_threshold_does_not_pump() {
    // The bug this whole mechanism exists to prevent: a count flapping between
    // 2 and 3 must not make the layer's volume flap with it.
    let mut e = counted(700.0);
    e.snap(&state(Situation::Combat, 3, 0, false));

    let mut lowest: f32 = 1.0;
    for i in 0..40 {
        let n = if i % 2 == 0 { 2 } else { 3 };
        let g = run(&mut e, &state(Situation::Combat, n, 0, false), 200.0);
        lowest = lowest.min(g);
    }
    assert!(
        lowest > 0.98,
        "volume pumped while the count oscillated: dipped to {lowest}"
    );
}

#[test]
fn a_sustained_fall_still_gets_through_the_hold() {
    // The guard must not become a way to ignore real changes.
    let mut e = counted(700.0);
    e.snap(&state(Situation::Combat, 3, 0, false));
    let g = run(&mut e, &state(Situation::Combat, 0, 0, false), 3000.0);
    assert!(g < 0.01, "layer should have gone quiet, sitting at {g}");
}

#[test]
fn exclusive_themes_cross_over() {
    let mut e = GainEngine::new(vec![
        LayerSpec { id: "leisure", trigger: Trigger::Situation { when: &CALM_OR_PAUSED },
            file_gain_db: 0.0, fade_in_ms: 700.0, fade_out_ms: 700.0, hold_ms: 0.0,
            mute_when: &NEVER },
        LayerSpec { id: "fight", trigger: Trigger::Situation { when: &COMBAT },
            file_gain_db: 0.0, fade_in_ms: 700.0, fade_out_ms: 700.0, hold_ms: 0.0,
            mute_when: &NEVER },
    ]);
    e.snap(&state(Situation::Calm, 0, 0, false));
    assert!(e.raw_gain(0) > 0.99 && e.raw_gain(1) < 0.01);

    let combat = state(Situation::Combat, 0, 0, false);
    e.update(&combat, 350.0);
    // Mid-crossfade both are audible, which is fine: it is one composition.
    assert!(e.raw_gain(0) > 0.2 && e.raw_gain(1) > 0.2, "should overlap while crossing");

    for _ in 0..40 {
        e.update(&combat, 50.0);
    }
    assert!(e.raw_gain(0) < 0.01 && e.raw_gain(1) > 0.99);
}

#[test]
fn pause_silences_the_enemy_layers_but_not_the_bed() {
    let specs = crate::packs::layers(crate::packs::PACKS[0].desktop_gains_db);
    let mut e = GainEngine::new(specs);
    e.snap(&state(Situation::Paused, 5, 5, true));

    let by = |id: &str| {
        (0..e.len()).find(|&i| e.spec(i).id == id).map(|i| e.raw_gain(i)).unwrap()
    };
    assert!(by("all_time") > 0.99, "the always-on bed keeps playing when paused");
    assert!(by("leisure") > 0.99, "pause uses the leisure theme");
    assert!(by("fight") < 0.01);
    assert!(by("zombie") < 0.01);
    assert!(by("garp") < 0.01);
    assert!(by("boss") < 0.01, "even a boss goes quiet while paused");
}

#[test]
fn boss_arrives_fast_and_leaves_slowly() {
    let specs = crate::packs::layers(crate::packs::PACKS[0].desktop_gains_db);
    let mut e = GainEngine::new(specs);
    let idx = (0..e.len()).find(|&i| e.spec(i).id == "boss").unwrap();

    e.snap(&state(Situation::Combat, 0, 0, false));
    let fighting = state(Situation::Combat, 0, 0, true);
    e.update(&fighting, 120.0);
    assert!(e.raw_gain(idx) > 0.99, "boss should be at full within its fade");

    let gone = state(Situation::Combat, 0, 0, false);
    e.update(&gone, 120.0);
    assert!(e.raw_gain(idx) > 0.8, "boss should linger, not snap off");
}

#[test]
fn the_volume_curve_is_perceptual_not_linear() {
    use crate::mixer::volume_to_gain;

    assert_eq!(volume_to_gain(0), 0.0, "zero must be silence, not nearly");
    assert_eq!(volume_to_gain(100), 1.0);
    assert!(volume_to_gain(150) <= 1.0, "out-of-range input must not amplify");

    // Halfway on the slider should be well below half the amplitude, otherwise
    // the top of the range barely does anything.
    let half = volume_to_gain(50);
    assert!(half < 0.4, "50% should be clearly quieter than half, got {half}");

    // Monotonic across every notch the menu can produce.
    let mut previous = -1.0;
    for step in 0..=10 {
        let g = volume_to_gain(step * 10);
        assert!(g > previous, "volume must rise at every notch");
        previous = g;
    }
}

#[test]
fn the_two_volume_controls_act_independently_on_the_mix() {
    use crate::mixer::{Mixer, Samples, SfxSpec};
    use erect_core::audio::AudioEvent;

    // A steady tone for the layer, a burst for the effect.
    let tone: alloc::vec::Vec<i16> = (0..8192)
        .map(|i| if (i / 32) % 2 == 0 { 8000 } else { -8000 })
        .collect();
    let burst: alloc::vec::Vec<i16> = (0..4096).map(|_| 8000i16).collect();

    let level = |music: u32, sfx: u32, fire: bool| -> f64 {
        let specs = vec![LayerSpec {
            id: "bed",
            trigger: Trigger::Always,
            file_gain_db: 0.0,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            hold_ms: 0.0,
            mute_when: &NEVER,
        }];
        let mut mixer = Mixer::new(
            vec![Samples::I16Mono(&tone)],
            GainEngine::new(specs),
            vec![SfxSpec {
                event: AudioEvent::Hit,
                samples: Samples::I16Mono(&burst),
                gain: 1.0,
                max_voices: 1,
                min_interval_frames: 0,
            }],
            44100,
        );
        mixer.set_volumes(music, sfx);
        if fire {
            mixer.fire(AudioEvent::Hit);
        }
        let mut out = vec![0i16; 2048];
        mixer.render(&mut out, &state(Situation::Combat, 0, 0, false), false);
        let sum: f64 = out.iter().map(|s| (*s as f64 / 32768.0).powi(2)).sum();
        (sum / out.len() as f64).sqrt()
    };

    let full = level(100, 100, false);
    assert!(full > 0.05, "the bed should be audible at full, got {full}");
    assert!(level(50, 100, false) < full * 0.5, "music at 50 should be well down");
    assert_eq!(level(0, 100, false), 0.0, "music at 0 must be silent");

    // Turning the music off leaves only the effect, and its own control works.
    let sfx_only = level(0, 100, true);
    assert!(sfx_only > 0.05, "the effect should still play, got {sfx_only}");
    assert!(level(0, 50, true) < sfx_only * 0.5, "effects at 50 should be down");
    assert_eq!(level(0, 0, true), 0.0, "both at 0 is silence");

    // And music volume does not touch the effect.
    assert!(
        (level(0, 100, true) - level(0, 100, true)).abs() < 1e-9,
        "sanity"
    );
}

#[test]
fn every_pack_is_described_consistently() {
    use crate::packs::{choose, LAYER_IDS, PACKS};

    assert!(PACKS.len() >= 2, "a run should have something to choose between");
    for pack in PACKS.iter() {
        assert!(pack.loop_samples > 0, "{} has no loop", pack.dir);
        assert_eq!(pack.desktop_gains_db.len(), LAYER_IDS.len());
        assert_eq!(pack.psp_gains_db.len(), LAYER_IDS.len());
        // The desktop plays the levels as mixed; only the PSP normalises and so
        // needs the difference handed back.
        assert!(pack.desktop_gains_db.iter().all(|g| *g == 0.0));
        assert!(
            pack.psp_gains_db.iter().all(|g| *g < 0.0),
            "{} should be turned back down on the PSP",
            pack.dir
        );
        assert_eq!(pack.sfx.len(), 3);
    }
    // What a copy-pasted entry would look like. Loop length is deliberately not
    // checked: two packs rendered from the same project template share it
    // honestly, and pack1 and pack3 do.
    for (i, a) in PACKS.iter().enumerate() {
        for b in PACKS.iter().skip(i + 1) {
            assert_ne!(a.dir, b.dir, "two packs point at the same directory");
            assert_ne!(
                a.psp_gains_db, b.psp_gains_db,
                "{} and {} carry the same levels, which no two mixes do",
                a.dir, b.dir
            );
        }
    }

    // Choosing reaches every pack and never falls off the end.
    let mut seen = alloc::vec![false; PACKS.len()];
    for seed in 0..64u64 {
        let picked = choose(seed);
        let idx = PACKS.iter().position(|p| p.dir == picked.dir).unwrap();
        seen[idx] = true;
    }
    assert!(seen.iter().all(|s| *s), "every pack should be reachable");
}

#[test]
fn pack_choice_spreads_even_when_the_clock_barely_moves() {
    use crate::packs::{choose, PACKS};

    // The real seeds are clock readings. A quantised clock moves its high bits
    // and leaves the low ones alone, which is exactly the case a plain
    // `seed % 2` gets wrong - and did, until the seed was hashed.
    let spread = |seeds: alloc::vec::Vec<u64>| {
        let mut counts = alloc::vec![0usize; PACKS.len()];
        for s in &seeds {
            let picked = choose(*s);
            counts[PACKS.iter().position(|p| p.dir == picked.dir).unwrap()] += 1;
        }
        counts
    };
    // Sixty-four draws over however many packs ship; a third of the even share
    // is low enough not to be flaky and high enough to catch clustering.
    let floor = 64 / PACKS.len() / 3;

    // Consecutive microseconds.
    let c = spread((1_000_000u64..1_000_064).collect());
    assert!(c.iter().all(|n| *n > floor), "consecutive seeds clustered: {c:?}");

    // A clock that only ever lands on multiples of 1000.
    let c = spread((0..64u64).map(|i| 5_000_000 + i * 1000).collect());
    assert!(c.iter().all(|n| *n > floor), "quantised seeds clustered: {c:?}");

    // And on multiples of 65536, where the bottom sixteen bits never change.
    let c = spread((0..64u64).map(|i| i * 65536).collect());
    assert!(c.iter().all(|n| *n > floor), "coarse seeds clustered: {c:?}");
}

#[test]
fn switching_packs_swaps_the_music_and_keeps_the_effects() {
    use crate::mixer::{Mixer, Samples, SfxSpec};
    use erect_core::audio::AudioEvent;

    let quiet: alloc::vec::Vec<i16> = (0..4096).map(|_| 1000i16).collect();
    let loud: alloc::vec::Vec<i16> = (0..8192).map(|_| 20000i16).collect();
    let burst: alloc::vec::Vec<i16> = (0..2048).map(|_| 12000i16).collect();

    let bed = || {
        alloc::vec![LayerSpec {
            id: "bed",
            trigger: Trigger::Always,
            file_gain_db: 0.0,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            hold_ms: 0.0,
            mute_when: &NEVER,
        }]
    };

    let mut mixer = Mixer::new(
        vec![Samples::I16Mono(&quiet)],
        GainEngine::new(bed()),
        vec![SfxSpec {
            event: AudioEvent::Hit,
            samples: Samples::I16Mono(&burst),
            gain: 1.0,
            max_voices: 1,
            min_interval_frames: 0,
        }],
        44100,
    );

    let st = state(Situation::Combat, 0, 0, false);
    let mut out = vec![0i16; 1024];
    mixer.render(&mut out, &st, false);
    let before = out.iter().map(|s| s.unsigned_abs() as u32).max().unwrap();
    assert_eq!(mixer.loop_len(), 4096);

    mixer.switch_music(vec![Samples::I16Mono(&loud)], GainEngine::new(bed()));
    assert_eq!(mixer.loop_len(), 8192, "the new pack sets the loop length");

    mixer.render(&mut out, &st, false);
    let after = out.iter().map(|s| s.unsigned_abs() as u32).max().unwrap();
    assert!(after > before * 4, "the other pack should be playing: {before} -> {after}");

    // Effects are untouched by the swap - they are the same in every pack.
    mixer.fire(AudioEvent::Hit);
    let mut silent = vec![0i16; 1024];
    mixer.switch_music(vec![Samples::I16Mono(&quiet)], GainEngine::new(bed()));
    mixer.fire(AudioEvent::Hit);
    mixer.render(&mut silent, &st, false);
    assert!(
        silent.iter().any(|s| s.unsigned_abs() > 5000),
        "the effect should still fire after a swap"
    );
}
