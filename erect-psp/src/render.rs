//! Draws the core's state with sceGu.
//!
//! Mirrors the desktop layout, with one deliberate difference: the PSP is
//! 480x272, so the smallest text sizes are raised. The percentage layout scales
//! on its own, but `hper(3.5)` would land at 9 px here and be unreadable.

use alloc::format;
use erect_core::color::Rgb;
use erect_core::config::*;
use erect_core::game::{Game, RunResult, State};
use erect_core::geom::Body;
use erect_core::waves::WaveRule;

use crate::gfx::{self, BLACK, SCREEN_H, SCREEN_W, WHITE};

const GREY: u32 = 0xffc8_c8c8;
const DIM: u32 = 0xff80_8080;
const PROJECTILE: u32 = 0xff22_22ff; // 0xAABBGGRR, so this is red

/// World x to screen x. The field is endless, so every world draw goes through
/// this; the HUD and the menus stay in screen coordinates.
fn on_screen(body: &Body, cam: f32) -> Body {
    Body { x: body.x - cam, ..*body }
}

/// Minimum readable text on a 480x272 panel.
fn readable(px: f32) -> f32 {
    px.max(11.0)
}

/// # Safety
/// Only valid between `gfx::begin_frame` and `gfx::end_frame`.
pub unsafe fn render(game: &Game) {
    unsafe {
        let v = &game.viewport;

        // Parallax skyline first, so the ground and ceiling bands trim it.
        let mut blocks = alloc::vec::Vec::new();
        for layer in 0..BACKDROP_LAYERS.len() {
            let shade = gfx::pack(game.backdrop_layer(layer, &mut blocks));
            for b in blocks.iter() {
                gfx::rect(b.x, b.y, b.w, b.h, shade);
            }
        }

        gfx::rect(
            0.0,
            v.hper(GROUND_Y_PCT),
            v.w,
            v.hper(FLOOR_H_PCT),
            BLACK,
        );
        gfx::rect(0.0, 0.0, v.w, v.hper(CEILING_H_PCT), BLACK);

        match game.state {
            // The result screen keeps the frozen battlefield behind it.
            State::Playing | State::Paused | State::ConfirmAbandon | State::GameOver => {
                render_session(game)
            }
            State::Settings => render_settings(game),
            State::Title => render_title(game),
        }

        if let Some(result) = game.result {
            render_result(game, &result);
        }
    }
}

/// Shown over the frozen battlefield after a run ends.
unsafe fn render_result(game: &Game, result: &RunResult) {
    unsafe {
        let v = &game.viewport;
        // No alpha fill here: a flat dim band behind the text costs one quad and
        // reads better on a 480x272 panel than a full-screen wash.
        gfx::rect(0.0, v.hper(22.0), v.w, v.hper(52.0), BLACK);

        centered("GAME OVER", 30.0, v.hper(9.0), WHITE, game);
        centered(&format!("SCORE {}", result.score), 46.0, v.hper(11.0), WHITE, game);
        centered(&format!("WAVE {}", result.wave), 58.0, v.hper(7.0), GREY, game);
        if result.is_record {
            centered("NEW RECORD", 68.0, v.hper(7.0), WHITE, game);
        }
        if game.awaiting_dismiss() {
            centered("PRESS X", 82.0, v.hper(6.0), GREY, game);
        }
    }
}

unsafe fn centered(text: &str, y_pct: f32, size: f32, color: u32, game: &Game) {
    let size = readable(size);
    let x = (SCREEN_W as f32 - crate::font::text_width(text, size)) / 2.0;
    unsafe {
        gfx::text_shadowed(x, game.viewport.hper(y_pct), size, color, text);
    }
}

unsafe fn render_title(game: &Game) {
    unsafe {
        let v = &game.viewport;
        // Say why it is quiet. Silence with no explanation is impossible to act
        // on, and the usual cause - a pack that never made it onto the Memory
        // Stick - is one the player can fix in a minute once they know.
        if let Some(note) = crate::sound::status().message() {
            centered(note, 94.0, v.hper(4.5), GREY, game);
        }
        // Both packs sit in memory at once; this says what that left behind.
        #[cfg(feature = "screenshot")]
        {
            let free = crate::sound::free_after_load();
            centered(
                &alloc::format!("FREE {} KB", free / 1024),
                88.0,
                v.hper(4.5),
                GREY,
                game,
            );
        }
        centered("E-Rect", 24.0, v.hper(13.0), WHITE, game);
        render_menu(game, game.title_menu.index, game.title_menu.top_pct);
        centered(
            &format!("RECORD: {}", game.settings.record_solo),
            80.0,
            v.hper(6.0),
            WHITE,
            game,
        );
    }
}

unsafe fn render_settings(game: &Game) {
    unsafe {
        let v = &game.viewport;
        centered("SETTINGS", 16.0, v.hper(10.0), WHITE, game);
        render_menu(game, game.settings_menu.index, game.settings_menu.top_pct);
        centered("LEFT/RIGHT CHANGE   O BACK", 88.0, v.hper(5.0), GREY, game);
    }
}

unsafe fn render_menu(game: &Game, selected: usize, top_pct: f32) {
    unsafe {
        let v = &game.viewport;
        let rows = game.menu_rows([true, false]);
        let size = readable(v.hper(8.0));

        for (i, row) in rows.iter().enumerate() {
            let is_selected = i == selected;
            let label = if !is_selected {
                row.label.clone()
            } else if row.is_adjustable() {
                format!("< {} >", row.label)
            } else {
                format!("> {} <", row.label)
            };
            let color = if is_selected { WHITE } else { GREY };
            let y_pct = top_pct + i as f32 * MENU_ROW_H_PCT;
            let text_w = crate::font::text_width(&label, size);
            let x = (SCREEN_W as f32 - text_w) / 2.0;
            gfx::text_shadowed(x, v.hper(y_pct), size, color, &label);

            if let Some(color_index) = row.swatch {
                let sw = v.wper(5.0);
                let sh = v.hper(4.0);
                let sx = x - sw - v.wper(2.0);
                let sy = v.hper(y_pct) - sh;
                gfx::rect(sx - 1.0, sy - 1.0, sw + 2.0, sh + 2.0, BLACK);
                gfx::rect(sx, sy, sw, sh, gfx::pack(Rgb::from_palette(color_index)));
            }
        }
    }
}

unsafe fn actor(body: &Body, color: u32, outline_w: f32, outline_h: f32) {
    unsafe {
        gfx::rect(
            body.x,
            body.y - outline_h,
            body.w + outline_w,
            body.h + outline_h * 2.0,
            BLACK,
        );
        gfx::rect(body.x, body.y, body.w, body.h, color);
    }
}

unsafe fn render_session(game: &Game) {
    unsafe {
        let v = &game.viewport;
        let cam = game.camera_x;
        let ow = v.wper(0.4);
        let oh = v.hper(0.4);

        if game.waves.countdown >= 0 {
            let text = format!("{}", game.waves.countdown);
            centered(&text, 36.0, v.hper(13.0), WHITE, game);
        }

        // Sights first, so an actor is never hidden behind one.
        for dot in game.aim_dots.iter() {
            let color = if dot.hot { PROJECTILE } else { WHITE };
            gfx::rect(dot.x - cam, dot.y, dot.size, dot.size, color);
        }

        for p in game.players.iter().filter(|p| !p.dead) {
            let body = on_screen(&p.body, cam);
            actor(&body, gfx::pack(p.color.shaded(p.hp / 255.0)), ow, oh);
            let gun = on_screen(&p.gun, cam);
            gfx::rect(gun.x, gun.y - oh, gun.w + ow, gun.h + oh * 2.0, BLACK);
            gfx::rect(gun.x, gun.y, gun.w, gun.h, WHITE);
        }

        // A blind wave hides the enemies themselves; the player, its attacks,
        // the blasts and the popups are all that is left to fight by.
        if game.waves.rule != WaveRule::Hidden {
            for z in game.zombies.iter() {
                actor(&on_screen(&z.body, cam), gfx::pack(z.color.shaded(z.hp / z.hpmax)), ow, oh);
            }
            for f in game.flyers.iter() {
                actor(&on_screen(&f.body, cam), gfx::pack(f.color.shaded(f.hp / f.hpmax)), ow, oh);
            }
        }

        // Field and explosions flicker on alternate phases, as in the original.
        if libm::sinf(game.timer as f32) < 0.0 {
            for p in game.players.iter().filter(|p| p.field.active) {
                let b = on_screen(&p.field.body, cam);
                gfx::rect(b.x - ow, b.y - oh, b.w + ow * 2.0, b.h + oh * 2.0, BLACK);
                gfx::rect(b.x, b.y, b.w, b.h, WHITE);
            }
            for e in game.explosions.iter().filter(|e| !e.finished) {
                let b = on_screen(&e.body, cam);
                gfx::rect(b.x, b.y - oh, b.w + ow, b.h + oh * 2.0, BLACK);
                gfx::rect(b.x, b.y, b.w, b.h, WHITE);
            }
        }

        for p in game.projectiles.iter() {
            let b = on_screen(&p.body, cam);
            gfx::rect(b.x - 1.0, b.y - 1.0, b.w + 2.0, b.h + 2.0, BLACK);
            gfx::rect(b.x, b.y, b.w, b.h, PROJECTILE);
        }

        // Repaint the top band so sprites never bleed into the HUD.
        gfx::rect(0.0, 0.0, v.w, v.hper(CEILING_H_PCT), BLACK);
        render_hud(game);

        let popup_size = readable(v.hper(9.0));
        for popup in game.popups.iter() {
            let lift = v.hper(popup.age(game.timer) as f32 / 20.0);
            gfx::text_shadowed(popup.x - cam, popup.y - lift, popup_size, WHITE, &popup.text);
        }

        match game.state {
            State::Paused => {
                centered("PAUSE", 30.0, v.hper(11.0), WHITE, game);
                render_menu(game, game.pause_menu.index, game.pause_menu.top_pct);
            }
            State::ConfirmAbandon => {
                centered("LEAVE THE RUN?", 28.0, v.hper(9.0), WHITE, game);
                centered("THE SCORE STILL COUNTS", 40.0, v.hper(5.0), GREY, game);
                render_menu(game, game.confirm_menu.index, game.confirm_menu.top_pct);
            }
            _ => {}
        }

        if game.waves.countdown >= 0 {
            // Both axes get announced; a wave can carry one of each.
            if let Some(label) = game.waves.kind.label() {
                centered(label, 52.0, v.hper(8.0), WHITE, game);
            }
            if let Some(label) = game.waves.rule.label() {
                centered(label, 60.0, v.hper(7.0), WHITE, game);
            }
        }
    }
}

unsafe fn render_hud(game: &Game) {
    unsafe {
        let v = &game.viewport;
        let p = &game.players[0];
        let needed = p.energy_needed(game.wave).max(1) as f32;
        let ratio = (p.energy as f32 / needed).clamp(0.0, 1.0);

        let bar_x = v.wper(66.0);
        let bar_w = v.wper(24.0);
        gfx::rect(
            bar_x,
            v.hper(2.5),
            bar_w,
            v.hper(5.0),
            gfx::pack(game.background.to_rgb()),
        );
        gfx::rect(bar_x, v.hper(2.5), bar_w * ratio + 1.0, v.hper(5.0), BLACK);
        gfx::rect(
            bar_x,
            v.hper(2.5),
            bar_w * ratio,
            v.hper(5.0),
            gfx::pack(p.color.shaded(p.hp / p.hpmax)),
        );

        let size = readable(v.hper(6.0));
        gfx::text(v.wper(1.0), v.hper(5.0), size, WHITE, &format!("S:{}", p.score));
        gfx::text(
            v.wper(1.0),
            v.hper(9.6),
            size,
            WHITE,
            &format!("W:{} K:{}/{}", game.wave, p.kills, game.wave * 10),
        );

        let charges = format!("x{}", p.super_charges);
        let cw = crate::font::text_width(&charges, size);
        gfx::text(
            (SCREEN_W as f32 - cw) / 2.0,
            v.hper(5.0),
            size,
            if p.dead { DIM } else { WHITE },
            &charges,
        );

        let _ = SCREEN_H;
    }
}
