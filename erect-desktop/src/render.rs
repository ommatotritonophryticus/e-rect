//! All drawing. Mirrors the canvas layout of the original: flat rectangles with
//! a 1-pixel-ish black outline, a black band top and bottom, and the same HUD
//! positions.

use erect_core::color::Rgb;
use erect_core::config::*;
use erect_core::game::{Game, RunResult, State};
use erect_core::geom::{Body, Viewport};
use erect_core::menu::Menu;
use erect_core::waves::WaveRule;
use macroquad::prelude::*;

/// World x to screen x. The field is endless, so everything except the HUD and
/// the menus has to come through here.
fn on_screen(body: &Body, cam: f32) -> Body {
    Body { x: body.x - cam, ..*body }
}

/// The core is platform-neutral about colour; macroquad wants normalised floats.
fn mq(rgb: Rgb) -> Color {
    let (r, g, b) = rgb.to_bytes();
    Color::from_rgba(r, g, b, 255)
}

/// The fraction of the em taken up by capitals in the face the game's text
/// sizes were originally tuned against.
///
/// Every `size_px` here is a percentage of the viewport picked by eye against
/// that face. Handing the number straight to macroquad as a font size makes it
/// mean something different in every font - a face with smaller capitals
/// shrinks the whole HUD without a single call site changing. Measuring the
/// current font and correcting for it keeps a given `size_px` the same height
/// on screen whatever the face. The PSP frontend normalises the same way.
const TUNED_CAP_RATIO: f32 = 0.8125;
/// Big enough that the rounding in an integer font size does not skew the
/// measurement it is taken from.
const CAP_PROBE_PX: u16 = 100;

pub struct Renderer {
    pub font: Font,
    /// `size_px` -> macroquad font size, for this font's proportions.
    size_correction: f32,
}

impl Renderer {
    pub fn new(font: Font) -> Self {
        // measure_text reports the height of the rendered glyph, so a capital
        // gives the cap height directly.
        let probe = measure_text("M", Some(&font), CAP_PROBE_PX, 1.0);
        let cap_ratio = probe.height / CAP_PROBE_PX as f32;
        let size_correction = if cap_ratio > 0.0 {
            TUNED_CAP_RATIO / cap_ratio
        } else {
            1.0
        };
        Self {
            font,
            size_correction,
        }
    }

    /// The font size to ask macroquad for so that `size_px` lands as tall as
    /// it did in the face the layout was tuned against.
    fn font_px(&self, size_px: f32) -> u16 {
        (size_px * self.size_correction).max(1.0) as u16
    }

    fn params(&self, size_px: f32, color: Color) -> TextParams<'_> {
        TextParams {
            font: Some(&self.font),
            font_size: self.font_px(size_px),
            color,
            ..Default::default()
        }
    }

    fn text_width(&self, text: &str, size_px: f32) -> f32 {
        measure_text(text, Some(&self.font), self.font_px(size_px), 1.0).width
    }

    fn draw_centered(&self, v: &Viewport, text: &str, y_pct: f32, size_px: f32, color: Color) {
        let x = (v.w - self.text_width(text, size_px)) / 2.0;
        draw_text_ex(text, x, v.hper(y_pct), self.params(size_px, color));
    }

    /// Centered text with the game's usual black drop shadow.
    fn draw_centered_shadowed(
        &self,
        v: &Viewport,
        text: &str,
        y_pct: f32,
        size_px: f32,
        color: Color,
    ) {
        let x = (v.w - self.text_width(text, size_px)) / 2.0;
        draw_text_ex(
            text,
            x + 3.0,
            v.hper(y_pct) + 3.0,
            self.params(size_px, BLACK),
        );
        draw_text_ex(text, x, v.hper(y_pct), self.params(size_px, color));
    }

    fn draw_right_aligned(
        &self,
        v: &Viewport,
        text: &str,
        right_x_pct: f32,
        y_pct: f32,
        size_px: f32,
        color: Color,
    ) {
        let x = v.wper(right_x_pct) - self.text_width(text, size_px);
        draw_text_ex(text, x, v.hper(y_pct), self.params(size_px, color));
    }

    /// Filled rect with the black outline every actor in this game has.
    fn draw_actor(&self, v: &Viewport, body: &Body, color: Color) {
        draw_rectangle(
            body.x,
            body.y - v.hper(0.1),
            body.w + v.wper(0.2),
            body.h + v.hper(0.2),
            BLACK,
        );
        draw_rectangle(body.x, body.y, body.w, body.h, color);
    }

    pub fn render(&self, game: &Game, pads_connected: [bool; MAX_PLAYERS]) {
        let v = &game.viewport;

        clear_background(mq(game.background.to_rgb()));

        // Parallax skyline, behind everything. Drawn before the ground and
        // ceiling bands so those trim it top and bottom for free.
        let mut blocks = Vec::new();
        for layer in 0..BACKDROP_LAYERS.len() {
            let shade = mq(game.backdrop_layer(layer, &mut blocks));
            for b in blocks.iter() {
                draw_rectangle(b.x, b.y, b.w, b.h, shade);
            }
        }

        draw_rectangle(0.0, v.hper(GROUND_Y_PCT), v.w, v.hper(FLOOR_H_PCT), BLACK);
        draw_rectangle(0.0, 0.0, v.w, v.hper(CEILING_H_PCT), BLACK);

        match game.state {
            // The result screen keeps the frozen battlefield behind it.
            State::Playing | State::Paused | State::ConfirmAbandon | State::GameOver => {
                self.render_session(game)
            }
            State::Settings => self.render_settings(game, pads_connected),
            State::DevMenu => self.render_dev_menu(game, pads_connected),
            State::Title => self.render_title(game, pads_connected),
        }

        if let Some(result) = game.result {
            self.render_result(game, &result);
        }
    }

    fn render_menu(&self, game: &Game, menu: &Menu, pads_connected: [bool; MAX_PLAYERS]) {
        let v = &game.viewport;
        let rows = game.menu_rows(pads_connected);
        let size = v.hper(6.0);

        for (i, row) in rows.iter().enumerate() {
            let selected = i == menu.index;
            let label = if !selected {
                row.label.clone()
            } else if row.is_adjustable() {
                // Adjustable rows advertise that left/right does something.
                format!("< {} >", row.label)
            } else {
                format!("> {} <", row.label)
            };
            let color = if selected {
                WHITE
            } else {
                Color::new(0.78, 0.78, 0.78, 1.0)
            };
            let y_pct = menu.row_y_pct(i);
            self.draw_centered_shadowed(v, &label, y_pct, size, color);

            if let Some(color_index) = row.swatch {
                let text_w = self.text_width(&label, size);
                let sw = v.wper(6.0);
                let sh = v.hper(3.5);
                let sx = (v.w - text_w) / 2.0 - sw - v.wper(2.0);
                let sy = v.hper(y_pct) - sh;
                draw_rectangle(sx - 2.0, sy - 2.0, sw + 4.0, sh + 4.0, BLACK);
                draw_rectangle(sx, sy, sw, sh, mq(Rgb::from_palette(color_index)));
            }
        }
    }

    fn render_title(&self, game: &Game, pads_connected: [bool; MAX_PLAYERS]) {
        let v = &game.viewport;
        self.draw_centered_shadowed(v, "E-Rect", 24.0, v.hper(10.0), WHITE);
        self.render_menu(game, &game.title_menu, pads_connected);

        let size = v.hper(4.5);
        self.draw_centered(
            v,
            &format!("RECORD 1P: {}", game.settings.record_solo),
            76.0,
            size,
            WHITE,
        );
        self.draw_centered(
            v,
            &format!("RECORD 2P: {}", game.settings.record_duo),
            82.0,
            size,
            WHITE,
        );
    }

    fn render_settings(&self, game: &Game, pads_connected: [bool; MAX_PLAYERS]) {
        let v = &game.viewport;
        self.draw_centered_shadowed(v, "SETTINGS", 16.0, v.hper(8.0), WHITE);
        self.render_menu(game, &game.settings_menu, pads_connected);

        let hint = Color::new(0.78, 0.78, 0.78, 1.0);
        let size = v.hper(3.5);
        self.draw_centered(v, "LEFT / RIGHT TO CHANGE", 78.0, size, hint);
        self.draw_centered(v, "ESC OR B TO GO BACK", 83.0, size, hint);
    }

    fn render_dev_menu(&self, game: &Game, pads_connected: [bool; MAX_PLAYERS]) {
        let v = &game.viewport;
        self.draw_centered_shadowed(v, "DEV", 16.0, v.hper(8.0), WHITE);
        self.render_menu(game, &game.dev_menu, pads_connected);

        let hint = Color::new(0.78, 0.78, 0.78, 1.0);
        let size = v.hper(3.5);
        self.draw_centered(v, "LEFT / RIGHT TO CHANGE", 84.0, size, hint);
        self.draw_centered(v, "ESC OR B TO GO BACK", 89.0, size, hint);
    }

    fn render_session(&self, game: &Game) {
        let v = &game.viewport;
        let cam = game.camera_x;

        if game.waves.countdown >= 0 {
            self.draw_centered_shadowed(
                v,
                &game.waves.countdown.to_string(),
                36.0,
                v.hper(10.0),
                WHITE,
            );
        }

        // Sights first, so an actor is never hidden behind one.
        for dot in game.aim_dots.iter() {
            let color = if dot.hot { Color::new(1.0, 0.13, 0.13, 1.0) } else { WHITE };
            draw_rectangle(dot.x - cam, dot.y, dot.size, dot.size, color);
        }

        for player in game.players.iter().filter(|p| !p.dead) {
            let body = on_screen(&player.body, cam);
            self.draw_actor(v, &body, mq(player.color.shaded(player.hp / 255.0)));
            // Melee box: white with a black outline, like the original.
            let gun = on_screen(&player.gun, cam);
            draw_rectangle(
                gun.x,
                gun.y - v.hper(0.1),
                gun.w + v.wper(0.2),
                gun.h + v.hper(0.2),
                BLACK,
            );
            draw_rectangle(gun.x, gun.y, gun.w, gun.h, mq(player.gun_color()));
        }

        // A blind wave hides the enemies themselves. The player, its attacks,
        // the blasts and the score popups all stay: those are the only things
        // left to read the fight by.
        if game.waves.rule != WaveRule::Hidden {
            // Husks first, so a shedder standing on one of its own is the thing
            // in front. They take their parent's colour darkened, which is what
            // says whose they are without a second entry in the palette.
            for z in game.zombies.iter() {
                for husk in z.husks.iter() {
                    self.draw_actor(v, &on_screen(husk, cam), mq(z.color.shaded(HUSK_SHADE)));
                }
            }
            for z in game.zombies.iter() {
                self.draw_actor(v, &on_screen(&z.body, cam), mq(z.color.shaded(z.hp / z.hpmax)));
            }
            for f in game.flyers.iter() {
                self.draw_actor(v, &on_screen(&f.body, cam), mq(f.color.shaded(f.hp / f.hpmax)));
            }
        }

        // The ultimate field and explosions flicker on alternate phases.
        let flicker = (game.timer as f32).sin() < 0.0;
        if flicker {
            for player in game.players.iter().filter(|p| p.field.active) {
                let b = on_screen(&player.field.body, cam);
                draw_rectangle(
                    b.x - v.wper(0.2),
                    b.y - v.hper(0.2),
                    b.w + v.wper(0.4),
                    b.h + v.hper(0.4),
                    Color::new(0.03, 0.03, 0.03, 1.0),
                );
                draw_rectangle(b.x, b.y, b.w, b.h, WHITE);
            }
            for e in game.explosions.iter().filter(|e| !e.finished) {
                let b = on_screen(&e.body, cam);
                draw_rectangle(
                    b.x,
                    b.y - v.hper(0.1),
                    b.w + v.wper(0.2),
                    b.h + v.hper(0.2),
                    Color::new(0.03, 0.03, 0.03, 1.0),
                );
                draw_rectangle(b.x, b.y, b.w, b.h, WHITE);
            }
        }

        for p in game.projectiles.iter() {
            let b = on_screen(&p.body, cam);
            draw_rectangle(b.x - 1.0, b.y - 1.0, b.w + 2.0, b.h + 2.0, BLACK);
            draw_rectangle(b.x, b.y, b.w, b.h, Color::new(1.0, 0.13, 0.13, 1.0));
        }

        // Redraw the top band so sprites never bleed into the HUD.
        draw_rectangle(0.0, 0.0, v.w, v.hper(CEILING_H_PCT), BLACK);

        if game.players.len() == 1 {
            self.render_hud_solo(game);
        } else {
            self.render_hud_duo(game);
        }

        let popup_size = v.hper(8.0);
        for popup in game.popups.iter() {
            let lift = v.hper(popup.age(game.timer) as f32 / 20.0);
            draw_text_ex(
                &popup.text,
                popup.x - cam + 3.0,
                popup.y - lift + 3.0,
                self.params(popup_size, BLACK),
            );
            draw_text_ex(
                &popup.text,
                popup.x - cam,
                popup.y - lift,
                self.params(popup_size, WHITE),
            );
        }

        match game.state {
            State::Paused => {
                self.draw_centered_shadowed(v, "pause", 32.0, v.hper(10.0), WHITE);
                self.render_menu(game, &game.pause_menu, [false; MAX_PLAYERS]);
            }
            State::ConfirmAbandon => {
                self.draw_centered_shadowed(v, "leave the run?", 32.0, v.hper(9.0), WHITE);
                self.draw_centered_shadowed(v, "the score still counts", 40.0, v.hper(5.0), GRAY);
                self.render_menu(game, &game.confirm_menu, [false; MAX_PLAYERS]);
            }
            _ => {}
        }

        if game.waves.countdown >= 0 {
            // Both axes get announced; a wave can carry one of each.
            if let Some(label) = game.waves.kind.label() {
                self.draw_centered_shadowed(v, label, 52.0, v.hper(9.0), WHITE);
            }
            if let Some(label) = game.waves.rule.label() {
                self.draw_centered_shadowed(v, label, 60.0, v.hper(8.0), WHITE);
            }
        }
    }

    /// Shown over the frozen battlefield after a run ends.
    fn render_result(&self, game: &Game, result: &RunResult) {
        let v = &game.viewport;
        // Dim the scene so the numbers read against it.
        draw_rectangle(0.0, 0.0, v.w, v.h, Color::new(0.0, 0.0, 0.0, 0.6));

        self.draw_centered_shadowed(v, "game over", 30.0, v.hper(11.0), WHITE);
        self.draw_centered_shadowed(
            v,
            &format!("score {}", result.score),
            44.0,
            v.hper(9.0),
            WHITE,
        );
        self.draw_centered_shadowed(
            v,
            &format!("wave {}", result.wave),
            56.0,
            v.hper(6.0),
            GRAY,
        );
        if result.is_record {
            self.draw_centered_shadowed(v, "new record", 68.0, v.hper(6.0), WHITE);
        }
        if game.awaiting_dismiss() {
            self.draw_centered_shadowed(v, "press space or enter", 82.0, v.hper(5.0), GRAY);
        }
    }

    /// Single-player HUD, laid out as it always has been.
    fn render_hud_solo(&self, game: &Game) {
        let v = &game.viewport;
        let p = &game.players[0];
        let needed = p.energy_needed(game.wave).max(1) as f32;
        let ratio = (p.energy as f32 / needed).clamp(0.0, 1.0);

        draw_rectangle(
            v.wper(70.0),
            v.hper(2.5),
            v.wper(20.0),
            v.hper(5.0),
            mq(game.background.to_rgb()),
        );
        draw_rectangle(
            v.wper(70.0),
            v.hper(2.5),
            v.wper(20.0) * ratio + v.wper(0.2),
            v.hper(5.0),
            BLACK,
        );
        draw_rectangle(
            v.wper(70.0),
            v.hper(2.5),
            v.wper(20.0) * ratio,
            v.hper(5.0),
            mq(p.color.shaded(p.hp / p.hpmax)),
        );

        let size = v.hper(5.0);
        draw_text_ex(
            format!("score:{}", p.score),
            v.wper(0.5),
            v.hper(4.5),
            self.params(size, WHITE),
        );
        draw_text_ex(
            format!("wave:{} kill:{}/{}", game.wave, p.kills, game.wave * 10),
            v.wper(0.5),
            v.hper(9.5),
            self.params(size, WHITE),
        );
        self.draw_centered(v, &p.super_charges.to_string(), 6.0, size, WHITE);
    }

    /// Two-player HUD: P1 left, shared wave info centred, P2 right.
    fn render_hud_duo(&self, game: &Game) {
        let v = &game.viewport;
        self.render_player_panel(game, 0, 0.5, false);
        self.render_player_panel(game, 1, 99.5, true);

        let size = v.hper(3.6);
        self.draw_centered(v, &format!("WAVE {}", game.wave), 4.2, size, WHITE);
        self.draw_centered(
            v,
            &format!("KILL {}/{}", game.total_kills(), game.wave * 10),
            8.6,
            size,
            WHITE,
        );
    }

    fn render_player_panel(&self, game: &Game, slot: usize, edge_x_pct: f32, align_right: bool) {
        let v = &game.viewport;
        let p = &game.players[slot];
        let size = v.hper(3.6);

        if p.dead {
            let text = format!("P{} OUT", p.index + 1);
            let grey = Color::new(0.5, 0.5, 0.5, 1.0);
            if align_right {
                self.draw_right_aligned(v, &text, edge_x_pct, 6.0, size, grey);
            } else {
                draw_text_ex(
                    &text,
                    v.wper(edge_x_pct),
                    v.hper(6.0),
                    self.params(size, grey),
                );
            }
            return;
        }

        let text = format!("P{} {}  x{}", p.index + 1, p.score, p.super_charges);
        let color = mq(p.color);
        if align_right {
            self.draw_right_aligned(v, &text, edge_x_pct, 4.2, size, color);
        } else {
            draw_text_ex(
                &text,
                v.wper(edge_x_pct),
                v.hper(4.2),
                self.params(size, color),
            );
        }

        let bar_w = v.wper(22.0);
        let bar_x = if align_right {
            v.wper(edge_x_pct) - bar_w
        } else {
            v.wper(edge_x_pct)
        };
        let bar_y = v.hper(5.4);
        let bar_h = v.hper(3.4);
        let needed = p.energy_needed(game.wave).max(1) as f32;
        let ratio = (p.energy as f32 / needed).clamp(0.0, 1.0);

        draw_rectangle(bar_x, bar_y, bar_w, bar_h, mq(game.background.to_rgb()));
        draw_rectangle(bar_x, bar_y, bar_w * ratio + v.wper(0.2), bar_h, BLACK);
        draw_rectangle(
            bar_x,
            bar_y,
            bar_w * ratio,
            bar_h,
            mq(p.color.shaded(p.hp / p.hpmax)),
        );
    }
}
