// A release build on Windows should not drag a console window along behind it.
// Debug builds keep it: that is where the "sound disabled" note goes.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

//! E-Rect - Rust port of the original canvas game.
//!
//! The browser version stepped its simulation once per rendered frame, so the
//! whole game ran at double speed on a 120 Hz display. Here the loop feeds a
//! fixed-rate accumulator instead: every tuning constant keeps the meaning it
//! had (they are all per-tick), but speed no longer depends on the monitor.

mod input;
mod persist;
mod render;
mod sound;

use erect_core::config::TICK_SECONDS;
use std::path::PathBuf;
use erect_core::game::Game;
use erect_core::geom::Viewport;
use input::InputReader;
use macroquad::prelude::*;
use render::Renderer;

fn window_conf() -> Conf {
    Conf {
        window_title: "E-Rect".to_owned(),
        window_width: 1024,
        window_height: 640,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    // Embedded so the binary is self-contained on every platform - no asset
    // directory to ship alongside it.
    let font = load_ttf_font_from_bytes(include_bytes!("../assets/font.ttf"))
        .expect("bundled font failed to load");

    let mut viewport = Viewport::new(screen_width(), screen_height());
    let mut settings = persist::load();
    // The launch counter is what guarantees a different run even where the
    // clock does not move; the PSP needs it, and it costs nothing here.
    settings.launches = settings.launches.wrapping_add(1);
    settings.dirty = true;
    let seed = (get_time() * 1_000_000.0) as u64
        ^ 0x5DEECE66D
        ^ settings.launches.rotate_left(42);
    let mut game = Game::new(viewport, seed, settings, &input::SCHEMES, 2);

    // Sound is optional: a missing pack or no output device must not stop play.
    let sound = match sound::Sound::start(&packs_dir(), seed) {
        Ok(s) => Some(s),
        Err(why) => {
            eprintln!("sound disabled: {why}");
            None
        }
    };
    let renderer = Renderer::new(font);
    let mut reader = InputReader::new();

    let mut accumulator = 0.0f64;
    let mut last = get_time();

    loop {
        viewport.sync(screen_width(), screen_height());
        game.viewport = viewport;
        game.gravity = viewport.hper(1.0);

        let now = get_time();
        let mut dt = now - last;
        last = now;
        // Clamp so a stall (window drag, breakpoint) cannot spiral into a long
        // catch-up burst of ticks.
        if dt > 0.25 {
            dt = 0.25;
        }
        accumulator += dt;

        let mut frame = reader.read(&game.player_schemes());
        let pads = frame.pads_connected;

        let mut ticked = false;
        while accumulator >= TICK_SECONDS {
            game.tick(&frame);
            accumulator -= TICK_SECONDS;
            if !ticked {
                // One-shot intents belong to exactly one tick.
                frame.clear_edges();
                ticked = true;
            }
        }

        if let Some(s) = &sound {
            s.set_roll(game.audio_roll);
            s.set_volumes(game.settings.music_volume, game.settings.sfx_volume);
            s.set_state(game.music_state());
            for event in game.audio.drain() {
                s.fire(event);
            }
        } else {
            game.audio.clear();
        }

        // The core flags changed settings; writing them is the platform's job.
        if game.settings.dirty {
            persist::save(&game.settings);
            game.settings.dirty = false;
        }

        if game.quit_requested {
            // Settings are already flushed above, so this is safe to do here.
            return;
        }

        renderer.render(&game, pads);
        next_frame().await;
    }
}

/// Looks for the packs next to the executable first, so a shipped build works,
/// then in the source tree for `cargo run`.
fn packs_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join("packs");
            if beside.is_dir() {
                return beside;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../packs")
}
