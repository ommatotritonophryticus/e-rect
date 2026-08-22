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
//!
//! One binary serves three shapes of machine. A desktop fills its window and
//! reads a keyboard; a browser gets the phone layout, a letterboxed field with
//! a pad drawn under it, and no sound at all - the mixer wants raw device
//! access, which is the one thing a first browser build does not have. Silence
//! was already survivable: the game has always treated sound as optional.

mod input;
mod persist;
mod render;
#[cfg(not(target_arch = "wasm32"))]
mod sound;
mod touch;
#[cfg(target_arch = "wasm32")]
mod sound_web;
#[cfg(all(feature = "harness", not(target_arch = "wasm32")))]
mod harness;

use erect_core::config::TICK_SECONDS;
use erect_core::game::Game;
use erect_core::geom::Viewport;
use erect_core::input::InputFrame;
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

/// Folds a second source of intents into the first. Nothing is exclusive: a
/// browser build on a laptop can be played with the keyboard and the drawn pad
/// at the same time, and neither has to know about the other.
fn merge(into: &mut InputFrame, from: &InputFrame) {
    for (dst, src) in into.players.iter_mut().zip(from.players.iter()) {
        dst.left |= src.left;
        dst.right |= src.right;
        dst.jump |= src.jump;
        dst.slam |= src.slam;
        dst.attack |= src.attack;
        dst.attack_held |= src.attack_held;
        dst.dash |= src.dash;
    }
    into.menu.up |= from.menu.up;
    into.menu.down |= from.menu.down;
    into.menu.left |= from.menu.left;
    into.menu.right |= from.menu.right;
    into.menu.confirm |= from.menu.confirm;
    into.menu.back |= from.menu.back;
    into.pause |= from.pause;
    into.dev_menu |= from.dev_menu;
}

/// The one-shot half of a frame, with the held half dropped.
///
/// Held state is re-read every frame and must not be carried; edges are the
/// opposite - they happen once and have to survive until a tick can take them.
fn edges_only(frame: &InputFrame) -> InputFrame {
    let mut kept = *frame;
    for p in kept.players.iter_mut() {
        p.left = false;
        p.right = false;
        p.attack_held = false;
    }
    kept
}

/// Anything at all from the player, for the one moment the game needs to know
/// only that something happened.
#[cfg(target_arch = "wasm32")]
fn touched() -> bool {
    !touches().is_empty()
        || is_mouse_button_pressed(MouseButton::Left)
        || get_last_key_pressed().is_some()
}

#[macroquad::main(window_conf)]
async fn main() {
    // Embedded so the binary is self-contained on every platform - no asset
    // directory to ship alongside it, which is also what makes it work from a
    // browser with nothing but the wasm file.
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
    #[cfg(not(target_arch = "wasm32"))]
    let sound = match sound::Sound::start(&sound::packs_dir(), seed) {
        Ok(s) => Some(s),
        Err(why) => {
            eprintln!("sound disabled: {why}");
            None
        }
    };

    let renderer = Renderer::new(font);
    let mut reader = InputReader::new();
    let mut pad = touch::TouchPad::new(screen_width(), screen_height());
    // A browser is assumed to be a phone until proven otherwise - and even on a
    // desktop browser the pad is what wants testing. Native builds start on the
    // full window and can switch with F2 to check the layout with a mouse.
    let mut phone_layout = cfg!(target_arch = "wasm32");

    // Unattended run, if one was asked for. Set up after the game exists and
    // before the first frame, so the dev parameters are in place by frame zero.
    #[cfg(all(feature = "harness", not(target_arch = "wasm32")))]
    let mut harness = harness::Harness::from_env(&mut game);
    #[cfg(all(feature = "harness", not(target_arch = "wasm32")))]
    let mut harness_frame = 0u32;

    // Fetch the soundtrack before the first frame of play. Nine files off the
    // network, a screen between each so the page is never just still.
    #[cfg(target_arch = "wasm32")]
    let mut sound = {
        // Wait to be touched before asking for a note. A browser keeps its
        // audio suspended until then, and a suspended context never finishes
        // decoding - so this is not a courtesy, it is the only order in which
        // the loading below can make progress at all.
        while !touched() {
            renderer.render_press_to_start(screen_width(), screen_height());
            next_frame().await;
        }

        let mut loader = sound_web::Loader::new(seed);
        while !loader.settled() {
            renderer.render_loading(
                screen_width(),
                screen_height(),
                loader.done(),
                loader.total(),
                loader.current(),
            );
            next_frame().await;
            loader.pump().await;
        }
        let why = loader.problem().map(|w| w.to_string());
        let count = loader.loaded();
        let sound = loader.finish();
        // Say so and carry on. Silence is a worse game, not a broken one - but
        // it should never be a silence nobody explained.
        if sound.is_none() {
            let why = why.unwrap_or_else(|| format!("only {count} files arrived"));
            for _ in 0..180 {
                renderer.render_sound_failed(screen_width(), screen_height(), &why);
                next_frame().await;
            }
        }
        sound
    };

    let mut accumulator = 0.0f64;
    let mut last = get_time();
    // Presses no tick has taken yet. A rendered frame does not always advance
    // the simulation: the browser draws at its own rate and the game steps at a
    // fixed 60, so some frames produce no tick at all. An edge landing on one of
    // those used to be thrown away, which is why roughly every third attack went
    // missing while the button visibly lit up.
    let mut pending = InputFrame::default();

    loop {
        let (win_w, win_h) = (screen_width(), screen_height());
        if is_key_pressed(KeyCode::F2) {
            phone_layout = !phone_layout;
        }
        pad.resize(win_w, win_h);
        let field = if phone_layout {
            pad.layout().field
        } else {
            Rect::new(0.0, 0.0, win_w, win_h)
        };

        viewport.sync(field.w, field.h);
        game.viewport = viewport;
        game.gravity = viewport.hper(1.0);

        let now = get_time();
        let mut dt = now - last;
        last = now;
        // Clamp so a stall (window drag, breakpoint, a backgrounded tab) cannot
        // spiral into a long catch-up burst of ticks.
        if dt > 0.25 {
            dt = 0.25;
        }
        accumulator += dt;

        let mut frame = reader.read(&game.player_schemes());
        if phone_layout {
            let touched = pad.read();
            merge(&mut frame, &touched);
        }
        // Anything the last frame raised but no tick consumed.
        merge(&mut frame, &pending);
        let pads = frame.pads_connected;

        let mut ticked = false;
        while accumulator >= TICK_SECONDS {
            #[cfg(all(feature = "harness", not(target_arch = "wasm32")))]
            if let Some(h) = harness.as_ref() {
                h.before_tick(&mut game);
                h.drive(&mut frame, &game, harness_frame);
            }
            game.tick(&frame);
            accumulator -= TICK_SECONDS;
            if !ticked {
                // One-shot intents belong to exactly one tick.
                frame.clear_edges();
                ticked = true;
            }
        }

        // Either a tick took the edges, or they wait for the next frame.
        pending = if ticked {
            InputFrame::default()
        } else {
            edges_only(&frame)
        };

        #[cfg(not(target_arch = "wasm32"))]
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
        #[cfg(target_arch = "wasm32")]
        if let Some(s) = sound.as_mut() {
            s.set_volumes(game.settings.music_volume, game.settings.sfx_volume);
            s.update(game.music_state(), (dt * 1000.0) as f32);
            for event in game.audio.drain() {
                s.fire(event);
            }
        } else {
            // Nothing listening: the queue still has to be drained or it grows
            // for the length of the run.
            game.audio.clear();
        }

        // The core flags changed settings; writing them is the platform's job.
        if game.settings.dirty {
            persist::save(&game.settings);
            game.settings.dirty = false;
        }

        if game.quit_requested {
            // Settings are already flushed above, so this is safe to do here.
            // A browser tab cannot be closed from inside, so there it simply
            // goes back to doing nothing but drawing.
            #[cfg(not(target_arch = "wasm32"))]
            return;
            #[cfg(target_arch = "wasm32")]
            {
                game.quit_requested = false;
            }
        }

        // Black first, then the field. The renderer fills its own viewport, so
        // on a phone everything outside the field stays the black the pad is
        // drawn on.
        clear_background(BLACK);
        if phone_layout {
            let camera = pad.layout().camera(win_h);
            set_camera(&camera);
            renderer.render(&game, pads);
            set_default_camera();
            pad.draw(&renderer.font);
        } else {
            renderer.render(&game, pads);
        }
        #[cfg(all(feature = "harness", not(target_arch = "wasm32")))]
        if let Some(h) = harness.as_mut() {
            // After drawing, before the swap: the framebuffer still holds this
            // frame.
            if h.after_frame(harness_frame, &game) {
                return;
            }
            harness_frame += 1;
        }

        next_frame().await;
    }
}
