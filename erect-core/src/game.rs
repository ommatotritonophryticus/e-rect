//! Game state and the whole simulation.
//!
//! Collision/combat loops index into `self.zombies[i]` rather than holding a
//! `&mut` across the body. That keeps the field borrows disjoint (enemies vs
//! players vs effect vectors) and lets the logic stay a line-by-line match for
//! the original instead of being restructured around a command buffer.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::attack::AttackKind;
use crate::boon::{WallMod, SHIELD_COOLDOWN_TICKS};
use crate::audio::{AudioEvent, AudioQueue, MusicState, Situation};
use crate::backdrop::BackdropBlock;
use crate::color::{EaseColor, Rgb};
use crate::config::*;
use crate::dev::DevSetup;
use crate::entities::*;
use crate::geom::{Body, Viewport};
use crate::input::InputFrame;
use crate::menu::{Menu, MenuAction, MenuRow};
use crate::recipe::{Recipe, BROOD_MAX};
use crate::offer::{Offer, OfferItem, OFFER_SCORE_STEP};
use crate::settings::{SchemeInfo, Settings, VolumeChannel};
use crate::waves::{BossKind, FlyerKind, GroundKind, WaveAction, WaveManager, WaveRule};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Title,
    Settings,
    /// Starting parameters for a run, reached by a chord from the title.
    DevMenu,
    Playing,
    Paused,
    /// "Are you sure?" over the paused game. Its own state rather than a flag,
    /// so the renderer and the input both know exactly what is on screen.
    ConfirmAbandon,
    /// The run has ended and the result is on screen. Nothing simulates and no
    /// input is taken, so a death cannot be skipped past by a held button.
    GameOver,
}

/// What the run came to, kept for the result screen after the run itself has
/// been torn down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub score: i64,
    pub wave: i64,
    pub players: usize,
    pub is_record: bool,
}

pub struct Game {
    pub viewport: Viewport,
    pub state: State,
    pub settings: Settings,
    /// What the rolled heavy on the field turned out to be, and when it
    /// arrived. A combination has no colour to be recognised by, so the one
    /// time it can be named is when it walks in.
    pub elite_notice: Option<(String, i64)>,
    pub rng: Rng,
    /// What rolled enemies and their young are drawn from. Kept apart from the
    /// main generator for the same reason the offer is: what the player does in
    /// a fight must not shift which enemy the next roll produces.
    pub elite_rng: Rng,

    pub players: Vec<Player>,
    pub zombies: Vec<Zombie>,
    pub flyers: Vec<Flyer>,
    pub explosions: Vec<Explosion>,
    pub projectiles: Vec<Projectile>,
    /// Squares the players threw. Enemy-facing, unlike `projectiles`.
    pub bullets: Vec<Bullet>,
    /// Boxes the players left standing.
    pub traps: Vec<Trap>,
    /// The three things standing in the lull waiting to be hit. `None` outside
    /// an offer, which is most of the time.
    pub offer: Option<Offer>,
    pub popups: Vec<ScorePopup>,
    /// Sound cues raised this tick; the frontend drains them after `tick`.
    pub audio: AudioQueue,
    /// Shooter sights, rebuilt every tick. Drawn as a dotted line because both
    /// renderers can only put down axis-aligned rectangles - a solid diagonal
    /// is not something either of them can express.
    pub aim_dots: Vec<AimDot>,
    /// Set while `state` is `GameOver`.
    pub result: Option<RunResult>,
    /// The player picked EXIT. Frontends poll this and shut down their own way:
    /// a PSP calls `sceKernelExitGame`, a desktop closes its window.
    pub quit_requested: bool,
    result_ticks: i32,

    pub background: EaseColor,
    pub timer: i64,
    pub wave: i64,
    pub spawn_count: i64,
    pub gravity: f32,

    pub waves: WaveManager,
    pub title_menu: Menu,
    pub settings_menu: Menu,
    pub pause_menu: Menu,
    pub confirm_menu: Menu,
    pub dev_menu: Menu,
    /// What the developer menu will start a run on. Kept across visits so a
    /// wave can be tried, died on, and tried again without retyping it.
    pub dev: DevSetup,

    /// Control schemes this platform offers, supplied by the frontend.
    pub schemes: &'static [SchemeInfo],
    /// How many players the hardware can seat. A PSP has one controller, so it
    /// passes 1 and the "2 PLAYERS" row disappears on its own.
    pub max_players: usize,

    /// World x currently at the left edge of the view. The field itself has no
    /// edges; this is the only thing that decides what is on screen.
    pub camera_x: f32,
    /// Seeds the parallax skyline, so two runs do not share one.
    pub backdrop_seed: u64,
    /// Re-rolled on arriving at the title screen and on starting a run. A
    /// frontend picks its sound pack from this, so the core never has to know
    /// how many packs exist or what they are called.
    pub audio_roll: u64,
    /// Which way the view slides behind the menus, +1 or -1. Re-rolled on every
    /// arrival at the title, so the screen is not the same one twice.
    menu_drift: f32,
    /// Previous state, only so the arrival at the title can be spotted once
    /// rather than every tick it stays there.
    last_state: State,
    /// Hands out swing and blast identities. One counter for the whole game
    /// rather than one per player: with a counter each, both players' fifth
    /// swings share a number, and an enemy hit by one would be counted as
    /// already hit by the other.
    next_strike: u32,
    /// Team score at which the next offer comes due.
    next_offer_score: i64,
    /// Its own generator, for the same reason the soundtrack has one: which
    /// three options turn up must not shift which waves and variants follow.
    offer_rng: Rng,
    /// Its own generator, deliberately. Drawing the soundtrack from the game's
    /// stream would shift every later decision - which waves, which variants,
    /// which specials - so a cosmetic feature would quietly reshuffle the run.
    audio_rng: Rng,
}

/// What reached an enemy. The damage and the throw both come from this rather
/// than from a flag, because there are now four things that can land a hit and
/// they agree on almost none of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HitSource {
    /// The wall. Its own rules entirely - see the boss fraction.
    Field,
    /// A held swing, whatever kind the player is carrying.
    Swing,
    Bullet(usize),
    Trap(usize),
}

/* ---------------- background ---------------- */

// One flat colour behind everything, eased between three settings. It is the
// largest area on screen and the only always-visible read on what the game is
// doing.

/// A wave is running.
const BG_FIGHT: Rgb = Rgb::new(240.0, 50.0, 20.0);

/// The lull between waves. Blue rather than the green it used to be, which
/// leaves green to mean the menu and nothing else.
const BG_CALM: Rgb = Rgb::new(80.0, 255.0, 130.0);

/// Menus and the title.
const BG_MENU: Rgb = Rgb::new(80.0, 255.0, 130.0);

impl Game {
    pub fn new(
        viewport: Viewport,
        seed: u64,
        mut settings: Settings,
        schemes: &'static [SchemeInfo],
        max_players: usize,
    ) -> Self {
        settings.sanitize(schemes.len());
        let max_players = max_players.clamp(1, MAX_PLAYERS);
        let mut game = Self {
            gravity: viewport.hper(1.0),
            viewport,
            state: State::Title,
            settings,
            elite_notice: None,
            rng: Rng::new(seed),
            players: Vec::new(),
            zombies: Vec::new(),
            flyers: Vec::new(),
            explosions: Vec::new(),
            projectiles: Vec::new(),
            bullets: Vec::new(),
            traps: Vec::new(),
            offer: None,
            popups: Vec::new(),
            audio: AudioQueue::default(),
            aim_dots: Vec::new(),
            result: None,
            quit_requested: false,
            result_ticks: 0,
            background: EaseColor::new(BG_MENU),
            timer: 0,
            wave: 1,
            spawn_count: 0,
            waves: WaveManager::default(),
            title_menu: Menu::new(42.0),
            settings_menu: Menu::new(26.0),
            pause_menu: Menu::new(48.0),
            confirm_menu: Menu::new(52.0),
            // Nine rows, so a tighter pitch than the rest.
            // Fourteen rows on a 480x272 panel. At the old 7% pitch the last
            // five would sit under the floor band; 5% puts the bottom row at
            // 81% and still leaves 11.6px of text in a 13.6px slot, which is
            // above the readable floor the PSP renderer holds everything to.
            dev_menu: Menu::tight(16.0, 5.0),
            dev: DevSetup::default(),
            schemes,
            max_players,
            camera_x: 0.0,
            backdrop_seed: seed ^ 0xB5AD_4ECE_DA1C_E2A9,
            audio_roll: seed,
            menu_drift: 1.0,
            next_strike: 1,
            next_offer_score: OFFER_SCORE_STEP,
            offer_rng: Rng::new(seed ^ 0x9E37_79B9_7F4A_7C15),
            elite_rng: Rng::new(seed ^ 0xB5AD_4ECE_DA1C_E2A9),
            last_state: State::Title,
            audio_rng: Rng::new(seed ^ 0x243F_6A88_85A3_08D3),
        };
        game.start_new_run(1);
        // start_new_run arms the sky for play, but nothing is being played yet -
        // the game opens on the title screen, which owns its own colour.
        game.background = EaseColor::new(BG_MENU);
        game
    }

    /* ---------------- helpers ---------------- */

    pub fn spawn_x_for(&self, index: usize, count: usize) -> f32 {
        if count == 1 {
            self.viewport.wper(50.0)
        } else if index == 0 {
            self.viewport.wper(35.0)
        } else {
            self.viewport.wper(65.0)
        }
    }

    pub fn living_count(&self) -> usize {
        self.players.iter().filter(|p| !p.dead).count()
    }

    /// Index of the closest living player, or `None` if everyone is down.
    pub fn nearest_player(&self, x: f32) -> Option<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.dead)
            .min_by(|(_, a), (_, b)| {
                (a.body.x - x)
                    .abs()
                    .partial_cmp(&(b.body.x - x).abs())
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }

    /// Difficulty scales off the combined score, so a solo run behaves exactly
    /// as it did before co-op existed.
    pub fn total_score(&self) -> i64 {
        self.players.iter().map(|p| p.score).sum()
    }

    pub fn total_kills(&self) -> u32 {
        self.players.iter().map(|p| p.kills).sum()
    }

    /// The next swing or blast identity.
    ///
    /// Never zero: an enemy that has never been hit carries zero, so a strike
    /// numbered zero would read as having already reached everything.
    fn take_strike(&mut self) -> u32 {
        let id = self.next_strike;
        self.next_strike = self.next_strike.wrapping_add(1).max(1);
        id
    }

    /// Draws a new soundtrack. The value means nothing here; a frontend maps it
    /// onto whichever packs it shipped.
    ///
    /// The menu's drift direction is drawn here too, from the same generator:
    /// both are cosmetic, both belong to an arrival at the title, and both must
    /// stay out of the run's own stream or a new soundtrack would reshuffle
    /// every wave after it.
    fn roll_audio(&mut self) {
        self.audio_roll = self.audio_rng.next_seed();
        self.menu_drift = if self.audio_rng.flip() { 1.0 } else { -1.0 };
    }

    /// What the music should be doing right now.
    pub fn music_state(&self) -> MusicState {
        let over = self.state == State::GameOver;
        MusicState {
            situation: match self.state {
                State::Paused => Situation::Paused,
                // A wave that has not started yet is still a lull.
                State::Playing if self.waves.countdown < 0 => Situation::Combat,
                _ => Situation::Calm,
            },
            // Frozen enemies on the result screen are scenery, not a threat.
            zombies: if over { 0 } else { self.zombies.len() },
            flyers: if over { 0 } else { self.flyers.len() },
            // A rolled heavy counts too. It is not a boss by the flag - that
            // one also decides how the wall treats it and what it pays - but it
            // is the same thing to listen to: one enemy the wave is about,
            // standing there taking eleven swings.
            boss: !over
                && (self.zombies.iter().any(|z| z.is_boss || z.elite)
                    || self.flyers.iter().any(|f| f.is_boss)),
        }
    }

    /// How many enemies this wave will put out, for the HUD to count against.
    ///
    /// `None` on a wave that spawns nothing but its boss. There the number to
    /// reach would be one, and the boss makes more enemies than that out of
    /// being hit - so a count against a target would run past it and read as
    /// broken. What ends that wave is killing the thing, not reaching a number.
    pub fn wave_kill_target(&self) -> Option<i64> {
        if self.wave == ROLLED_BOSS_WAVE {
            None
        } else {
            Some(wave_budget(self.wave))
        }
    }

    pub fn player_schemes(&self) -> Vec<usize> {
        self.players.iter().map(|p| p.scheme).collect()
    }

    /* ---------------- lifecycle ---------------- */

    pub fn start_new_run(&mut self, player_count: usize) {
        self.roll_audio();
        self.camera_x = 0.0;
        let player_count = player_count.clamp(1, self.max_players);
        self.players.clear();
        for i in 0..player_count {
            let cfg = self.settings.players[i];
            let mut player = Player::new(
                i,
                &self.viewport,
                Rgb::from_palette(cfg.color_index),
                cfg.scheme,
            );
            let spawn_x = self.spawn_x_for(i, player_count);
            player.reset(&self.viewport, spawn_x);
            self.players.push(player);
        }
        self.background = EaseColor::new(BG_CALM);
        self.zombies.clear();
        self.flyers.clear();
        self.explosions.clear();
        self.projectiles.clear();
        self.bullets.clear();
        self.traps.clear();
        self.offer = None;
        self.next_offer_score = OFFER_SCORE_STEP;
        self.popups.clear();
        self.elite_notice = None;
        self.timer = 0;
        self.wave = 1;
        self.waves.begin_wave(1, &mut self.rng);
        self.spawn_count = 0;
        self.waves.reset();
    }

    /// A switch as the menu prints it.
    fn on_off(on: bool) -> &'static str {
        if on {
            "ON"
        } else {
            "OFF"
        }
    }

    /// Opens the developer menu, and clears whatever the last visit pinned so
    /// it cannot leak into a run started from the ordinary menu.
    fn open_dev_menu(&mut self) {
        self.state = State::DevMenu;
        self.dev_menu.index = 0;
    }

    /// Starts a run on the developer menu's parameters.
    ///
    /// All of it is applied *after* `start_new_run`, which resets the players
    /// and the wave manager - anything set before would be quietly undone.
    pub fn start_dev_run(&mut self) {
        let count = self.dev.players.clamp(1, self.max_players);
        self.start_new_run(count);

        self.wave = self.dev.wave.max(1);
        // The whole score goes to the first player, so the team total is the
        // number the menu was showing.
        if let Some(p) = self.players.first_mut() {
            p.score = self.dev.score;
        }

        for p in self.players.iter_mut() {
            p.attack = self.dev.attack;
            p.attack_level = self.dev.attack_level;
            p.boons = self.dev.boons;
        }

        self.waves.forced_kind = self.dev.kind;
        self.waves.forced_rule = self.dev.rule;
        // Settle the opening wave again now the pins are in: start_new_run
        // decided it without them, and on a different wave number.
        let wave = self.wave;
        self.waves.begin_wave(wave, &mut self.rng);

        self.state = State::Playing;
    }

    pub fn start_run(&mut self, player_count: usize) {
        let player_count = player_count.clamp(1, self.max_players);
        self.start_new_run(player_count);
        // An ordinary run never inherits the developer menu's pins.
        self.waves.forced_kind = None;
        self.waves.forced_rule = None;
        self.state = State::Playing;
    }

    pub fn toggle_pause(&mut self) {
        self.state = match self.state {
            State::Playing => {
                self.pause_menu.index = 0;
                State::Paused
            }
            State::Paused => State::Playing,
            other => other,
        };
    }

    fn on_wave_cleared(&mut self) {
        self.wave += 1;
        self.spawn_count = 0;
        self.background.set_target(BG_CALM);
        let count = self.players.len();
        for i in 0..count {
            self.players[i].kills = 0;
            if self.players[i].dead {
                // Downed players rejoin next wave so a co-op run does not leave
                // someone watching for the rest of the session.
                let spawn_x = self.spawn_x_for(i, count);
                let v = self.viewport;
                self.players[i].revive(&v, spawn_x);
            } else {
                self.players[i].super_charges += 1;
                self.players[i].hp += 32.0;
            }
        }
        self.waves.begin_wave(self.wave, &mut self.rng);
        self.waves.begin_countdown(WAVE_COUNTDOWN_SECONDS);
        self.maybe_offer();

        // The rule for the coming wave is settled by now, so a wave that pins
        // the player in one place can hand back full health first.
        // An offer pins the view the same way a held wave does, so the three
        // options cannot be walked away from and lost off the edge.
        if self.waves.rule == WaveRule::StaticCamera || self.offer.is_some() {
            for p in self.players.iter_mut() {
                p.hp = p.hpmax;
            }
        }
    }

    fn game_over(&mut self) {
        let team_score = self.total_score();
        let count = self.players.len();
        let is_record = team_score > self.settings.record(count);
        if is_record {
            self.settings.set_record(count, team_score);
        }
        self.result = Some(RunResult {
            score: team_score,
            wave: self.wave,
            players: count,
            is_record,
        });
        self.result_ticks = GAME_OVER_TICKS;
        self.state = State::GameOver;
    }

    /// True once the result screen will accept a key. Frontends show their
    /// "press to continue" prompt exactly while this holds.
    pub fn awaiting_dismiss(&self) -> bool {
        self.state == State::GameOver && self.result_ticks <= 0
    }

    /// Leaves the result screen once the player dismisses it.
    fn finish_game_over(&mut self) {
        let count = self.players.len();
        self.result = None;
        self.state = State::Title;
        self.title_menu.index = 0;
        self.start_new_run(count);
    }

    /// Moves enemies that wandered too far back to the spawn ring.
    ///
    /// On an open field a slow enemy that lost the player would otherwise keep
    /// the wave open forever, since a wave only clears when nothing is alive.
    /// Recycling rather than killing keeps the wave's budget honest.
    fn recycle_stragglers(&mut self) {
        let v = self.viewport;
        let limit = v.w * ENEMY_RECYCLE_SCREENS;
        for i in 0..self.zombies.len() {
            let Some(target) = self.nearest_player(self.zombies[i].body.x) else {
                return;
            };
            let px = self.players[target].body.center_x();
            let dx = self.zombies[i].body.center_x() - px;
            if libm::fabsf(dx) > limit {
                // Bring it back in on the side it drifted off towards.
                let side = if dx > 0.0 { 1.0 } else { -1.0 };
                self.zombies[i].body.x = px + side * (v.w / 2.0 + v.wper(5.0));
            }
        }
    }

    /// Centres the view on the players and keeps a co-op pair within one screen.
    ///
    /// The leash exists because the field is open but the screen is not: two
    /// players who walked apart would end up off opposite edges.
    fn update_camera(&mut self) {
        let v = self.viewport;

        // Behind a menu there is nobody to follow, so the view drifts instead.
        // The skyline is parallaxed off the camera, which makes this the only
        // thing standing between the title screen and a still image.
        if matches!(self.state, State::Title | State::Settings | State::DevMenu) {
            self.camera_x += self.menu_drift * v.wper(PLAYER_MOVE_PCT * MENU_CAMERA_DRIFT);
            return;
        }

        if self.players.is_empty() {
            return;
        }

        if self.players.len() > 1 {
            let span = v.wper(PLAYER_LEASH_PCT);
            let (a, b) = (self.players[0].body.center_x(), self.players[1].body.center_x());
            if (a - b).abs() > span {
                let mid = (a + b) / 2.0;
                let half = span / 2.0;
                for i in 0..self.players.len() {
                    let c = self.players[i].body.center_x();
                    let want = if c < mid { mid - half } else { mid + half };
                    self.players[i].body.x += want - c;
                    // Being stopped by the leash should not leave a throw
                    // pushing against it.
                    self.players[i].knockback_x = 0.0;
                }
            }
        }

        let focus: f32 = self
            .players
            .iter()
            .filter(|p| !p.dead)
            .map(|p| p.body.center_x())
            .sum::<f32>()
            / self.players.iter().filter(|p| !p.dead).count().max(1) as f32;

        // A held wave freezes the view where it started. Only the player needs
        // walls for that: ground enemies walk in on their own and flyers are
        // clamped to the view regardless.
        // An offer pins the view the same way a held wave does, so the three
        // options cannot be walked away from and lost off the edge.
        if self.waves.rule == WaveRule::StaticCamera || self.offer.is_some() {
            for i in 0..self.players.len() {
                let w = self.players[i].body.w;
                let x = self.players[i].body.x;
                let clamped = x.clamp(self.camera_x, self.camera_x + v.w - w);
                if clamped != x {
                    self.players[i].body.x = clamped;
                    self.players[i].knockback_x = 0.0;
                }
            }
            return;
        }

        let target = focus - v.w / 2.0;
        self.camera_x += (target - self.camera_x) * CAMERA_FOLLOW;
    }

    /// The colour a plain enemy wears: one of the players', picked at random
    /// when there are two of them.
    ///
    /// Base enemies used to roll their own out of 100..=255 on every channel -
    /// the same range the variants' signature colours live in - so one could
    /// arrive wearing a jumper's orange and lie to the player about what it was
    /// going to do. Colour is the only thing telling enemies apart here, which
    /// makes that the whole vocabulary being undermined. A player's colour can
    /// never be mistaken for a variant, and it is already on screen.
    fn plain_enemy_color(&mut self) -> Rgb {
        match self.players.len() {
            0 => Rgb::new(255.0, 255.0, 255.0),
            1 => self.players[0].color,
            n => {
                let pick = self.rng.range(0, n as i32 - 1).clamp(0, n as i32 - 1) as usize;
                self.players[pick].color
            }
        }
    }

    /// Stands three options up in the lull, if the team has earned one.
    ///
    /// Both players share it and either can take it: the trigger is the team
    /// total, which is the same number that drives how fast enemies chase, so
    /// what the run buys and what it faces keep step.
    fn maybe_offer(&mut self) {
        if self.total_score() < self.next_offer_score || self.players.is_empty() {
            return;
        }
        self.next_offer_score += OFFER_SCORE_STEP;
        let v = self.viewport;
        let ground = v.hper(GROUND_Y_PCT);
        let camera = self.camera_x;
        let (kind, level) = (self.players[0].attack, self.players[0].attack_level);
        // What is already held comes off the table. Boons are one-time, so
        // offering one twice would be offering nothing.
        let held = self.players[0].boons;
        let timer = self.timer;
        self.offer = Some(Offer::roll(
            &v,
            camera,
            ground,
            kind,
            level,
            held,
            timer,
            &mut self.offer_rng,
        ));
    }

    /// Hands the choice to everyone and takes the offer down.
    fn take_offer(&mut self, index: usize) {
        let Some(offer) = self.offer.as_ref() else {
            return;
        };
        let choice = offer.choices[index];
        let v = self.viewport;
        let (x, y) = (choice.body.x, choice.body.y);
        // Everyone gets it: one offer stands in the lull and either player may
        // be the one to reach it.
        for p in self.players.iter_mut() {
            match choice.item {
                OfferItem::Attack { kind, level } => {
                    p.attack = kind;
                    p.attack_level = level;
                }
                OfferItem::Boon(boon) => p.boons.take(boon),
            }
        }
        self.explosions.push(Explosion::new(x, y, &v));
        self.audio.push(AudioEvent::Slam);
        self.offer = None;
    }

    /// Checks the standing options against everything that can land a hit.
    ///
    /// They are not enemies - they never chase, never touch back, and never
    /// count towards clearing a wave - but they are hit exactly like one, so a
    /// bullet or a trap takes them just as a swing does.
    fn update_offer(&mut self) {
        // The lull ending takes the choice off the table. Skipping the wave in
        // is a way of declining, and a deliberate one.
        if !self.waves.between_waves() {
            self.offer = None;
            return;
        }
        let Some(offer) = self.offer.as_ref() else {
            return;
        };
        // They stand there inert for a moment first: a wave ends mid-swing, and
        // the options appear inside that swing.
        if !offer.armed(self.timer) {
            return;
        }
        let bodies: [Body; crate::offer::OFFER_CHOICES] =
            core::array::from_fn(|i| offer.choices[i].body);
        for (i, body) in bodies.iter().enumerate() {
            if self.find_attacker(body).is_some() {
                self.take_offer(i);
                return;
            }
        }
    }

    /// The sky the run itself wants: daylight in the lull, combat while a wave
    /// is up. Called when a menu closes and gives the sky back.
    fn restore_wave_sky(&mut self) {
        let quiet =
            self.waves.between_waves() || (self.zombies.is_empty() && self.flyers.is_empty());
        if quiet {
            self.background.set_target(BG_CALM);
        } else {
            self.background.set_target(BG_FIGHT);
        }
    }

    /// Fills `out` with one skyline layer and returns the colour to draw it in.
    ///
    /// Layers come farthest first, so a renderer that walks them in order gets
    /// the painter's algorithm for free. The colour is derived from the live
    /// background rather than a constant, which makes the whole skyline ease
    /// along with the wave-clear and combat colour changes on its own.
    pub fn backdrop_layer(&self, index: usize, out: &mut Vec<BackdropBlock>) -> Rgb {
        let (parallax, shade) = BACKDROP_LAYERS[index];
        // Each layer needs its own seed, or all three would draw one skyline
        // sliding at different speeds.
        let seed = self.backdrop_seed ^ (index as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        crate::backdrop::visible_blocks(self.camera_x, &self.viewport, seed, parallax, out);
        self.background.to_rgb().shaded(shade)
    }

    /// Left and right world edges of what the player can currently see.
    pub fn view_left(&self) -> f32 {
        self.camera_x
    }

    pub fn view_right(&self) -> f32 {
        self.camera_x + self.viewport.w
    }

    /// Banks energy from a hit or a kill, scaled by how hurt the player is.
    ///
    /// A player on full health gets face value; one on the brink gets three
    /// times as much, ramping linearly in between. The wounded are exactly who
    /// need a wall soonest, and this is the only place it is worked out - the
    /// four sources of energy would otherwise each need their own copy.
    ///
    /// Score is deliberately left alone: only the wall charges come faster, the
    /// scoreboard does not reward getting hurt.
    fn award_energy(&mut self, pi: usize, amount: i64) {
        let p = &self.players[pi];
        let ratio = if p.hpmax > 0.0 {
            (p.hp / p.hpmax).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let scale = 1.0 + ENERGY_DESPERATION_BONUS * (1.0 - ratio);
        self.players[pi].energy += libm::roundf(amount as f32 * scale) as i64;
    }

    /// Takes the price of a dash out of the energy bar, breaking a charge if the
    /// bar is short. Returns false when neither can pay.
    ///
    /// Breaking a charge counts as spending one, exactly as the wall does. Skip
    /// that and the fallback would be *cheaper* than paying properly: the next
    /// charge is priced on how many are held, so quietly dropping one would
    /// discount it.
    fn pay_for_dash(&mut self, i: usize) -> bool {
        let needed = self.players[i].energy_needed(self.wave);
        // Integer ceiling, so a third of a bar is never rounded down to free.
        let cost = ((needed + DASH_COST_DIVISOR - 1) / DASH_COST_DIVISOR).max(1);

        if self.players[i].energy >= cost {
            self.players[i].energy -= cost;
            return true;
        }
        if self.players[i].super_charges > 0 {
            self.players[i].super_charges -= 1;
            self.players[i].attacks_since_power_up += 1;
            // A charge is worth a full bar; the dash comes out of the change.
            self.players[i].energy = (self.players[i].energy + needed - cost).max(0);
            return true;
        }
        false
    }

    /// Applies one hit to a player: scaling, invulnerability, knockback and the
    /// death check, in one place.
    ///
    /// These four steps used to be copied at every source of damage, which made
    /// a per-wave rule about damage a four-site edit with nothing to catch the
    /// site you forgot. Note what is *not* here: whether the wall blocks the
    /// hit. That guard stays at the call sites, because today it only covers
    /// melee, and folding it in would silently start blocking projectiles and
    /// blasts too - a balance change nobody asked for.
    ///
    /// Returns true when this hit ended the run.
    pub(crate) fn damage_player(
        &mut self,
        pi: usize,
        amount: f32,
        source: &Body,
        reach: Reach,
    ) -> bool {
        if self.players[pi].untouchable(self.timer, reach) {
            // Still thrown about, just unhurt: being untouchable should not
            // also make the player immovable.
            //
            // A dash is the exception. It is a committed trajectory - no
            // steering, no second dash out of the first - and a throw would set
            // `ay` and lift the player straight out of their own dash. The
            // melee path has always refused to move a dashing player; this is
            // the same rule reaching the sources that never went through it.
            if !self.players[pi].in_dash_window(self.timer) {
                self.knock_back(pi, source);
            }
            return false;
        }
        // Absorbs the touch, then goes down for a second. Checked after every
        // untouchable case above, so nothing that could not have landed anyway
        // spends it - a shield is for the hits that would otherwise get through. The throw still happens: being shielded should
        // not also mean being immovable, and the throw is what carries the
        // player out of reach of the next one.
        if self.players[pi].boons.shield && self.timer >= self.players[pi].shield_ready_at {
            self.players[pi].shield_ready_at = self.timer + SHIELD_COOLDOWN_TICKS;
            self.knock_back(pi, source);
            return false;
        }
        self.players[pi].hp -= amount * self.waves.rule.damage_scale();
        self.knock_back(pi, source);
        self.players[pi].hp < 0.0 && self.down_player(pi)
    }

    /// A modified wall takes hold of the whole field the moment it goes up.
    ///
    /// One shove, not a standing force: every enemy is handed a short burst in
    /// one direction and then goes back to doing whatever it was doing. A
    /// lasting pull would be a tractor beam and a lasting push would make the
    /// wall unapproachable - neither is a wall.
    ///
    /// Called from both places a wall can be raised, which is why it is a
    /// method rather than a few lines at each: there are two, and a third would
    /// be easy to add without noticing this.
    fn wall_shove(&mut self, pi: usize) {
        let wall = self.players[pi].boons.wall;
        if wall == WallMod::Plain {
            return;
        }
        let centre = self.players[pi].body.x + self.players[pi].body.w / 2.0;
        // Pull sends each enemy toward that point, push sends it the other way;
        // both are read off which side the enemy is standing on.
        let sign = if wall == WallMod::Pull { 1.0 } else { -1.0 };
        let speed = self.viewport.wper(WALL_SHOVE_AX_PCT);
        for z in self.zombies.iter_mut() {
            let towards = if centre > z.body.x + z.body.w / 2.0 { 1.0 } else { -1.0 };
            z.ax = towards * sign * speed;
        }
        for f in self.flyers.iter_mut() {
            let towards = if centre > f.body.x + f.body.w / 2.0 { 1.0 } else { -1.0 };
            f.ax = towards * sign * speed;
        }
    }

    /// Throws a player up and away from whatever just hit them.
    ///
    /// Compares centres rather than left edges, so a wide enemy overlapping the
    /// player still pushes them out the side they are actually on.
    fn knock_back(&mut self, pi: usize, source: &Body) {
        let v = self.viewport;
        let p = &self.players[pi];
        let source_centre = source.x + source.w / 2.0;
        let player_centre = p.body.x + p.body.w / 2.0;
        let away = if source_centre > player_centre { -1.0 } else { 1.0 };
        self.players[pi].knockback_x = away * v.wper(KNOCKBACK_AWAY_PCT);
        self.players[pi].ay = -v.hper(KNOCKBACK_UP_PCT);
    }

    /// Returns true when this death ended the run.
    fn down_player(&mut self, index: usize) -> bool {
        self.players[index].dead = true;
        self.players[index].attack_ticks = 0;
        self.players[index].field = UltimateField::default();
        let (x, y) = (self.players[index].body.x, self.players[index].body.y);
        self.explosions.push(Explosion::new(x, y, &self.viewport));
        if self.living_count() == 0 {
            self.game_over();
            return true;
        }
        false
    }

    /// The rolled heavy's name, while it is still worth showing.
    pub fn elite_notice(&self) -> Option<&str> {
        self.elite_notice
            .as_ref()
            // The clock goes back to zero at the start of a run, so an age is
            // only meaningful forwards. Read the other way a stale notice looks
            // arbitrarily fresh.
            .filter(|(_, at)| (0..ELITE_NOTICE_TICKS).contains(&(self.timer - at)))
            .map(|(text, _)| text.as_str())
    }

    /* ---------------- per-tick entry point ---------------- */

    pub fn tick(&mut self, input: &InputFrame) {
        // Arriving at the title draws a new soundtrack, wherever it was arrived
        // from - abandoning a run, dying, backing out of settings. Spotted here
        // rather than at each of those sites so no path can forget.
        if self.state == State::Title && self.last_state != State::Title {
            self.roll_audio();
        }
        // The menus own the sky while they are up, and hand it back to whatever
        // the wave logic last asked for on the way out. Set from the state each
        // tick rather than at every transition into and out of a menu, because
        // there are six such transitions and only one of them is obvious.
        match self.state {
            State::Title | State::Settings | State::DevMenu => {
                self.background.set_target(BG_MENU)
            }
            _ if matches!(
                self.last_state,
                State::Title | State::Settings | State::DevMenu
            ) =>
            {
                self.restore_wave_sky();
            }
            _ => {}
        }
        self.last_state = self.state;

        self.background.approach(10.0);

        match self.state {
            State::Title | State::Settings | State::DevMenu => {
                // A chord from the title, and only from the title: it must not
                // be reachable from inside settings or a paused run.
                if input.dev_menu && self.state == State::Title {
                    self.open_dev_menu();
                }
                self.handle_menu_input(input);
                // Nothing else moves here, so the drift is the whole of what
                // makes the screen behind the menu alive.
                self.update_camera();
            }
            State::Paused => {
                if input.pause {
                    self.toggle_pause();
                } else {
                    self.handle_menu_input(input);
                }
                self.update_presentation();
            }
            State::ConfirmAbandon => {
                // Start does not unpause from here; the question has to be
                // answered, or backed out of.
                self.handle_menu_input(input);
                self.update_presentation();
            }
            State::GameOver => {
                // Everything is frozen. Input is refused until the hold expires,
                // so a button held at the moment of death cannot skip the
                // result; after that the player dismisses it themselves.
                if self.result_ticks > 0 {
                    self.result_ticks -= 1;
                } else if input.menu.confirm || input.menu.back {
                    self.finish_game_over();
                }
            }
            State::Playing => {
                if input.pause {
                    self.toggle_pause();
                    return;
                }
                self.apply_player_input(input);
                self.update_presentation();
                self.update_simulation();
            }
        }
    }

    fn handle_menu_input(&mut self, input: &InputFrame) {
        let m = input.menu;
        if m.up {
            self.menu_move(-1);
        }
        if m.down {
            self.menu_move(1);
        }
        if m.left {
            self.menu_adjust(-1);
        }
        if m.right {
            self.menu_adjust(1);
        }
        if m.confirm {
            self.menu_confirm();
        }
        if m.back {
            match self.state {
                State::Settings => {
                    self.state = State::Title;
                    self.title_menu.index = 0;
                }
                // Backing out of the question is the same as answering "no".
                State::ConfirmAbandon => {
                    self.state = State::Paused;
                    self.pause_menu.index = 0;
                }
                _ => {}
            }
        }
    }

    /* ---------------- menus ---------------- */

    /// Rows of the currently shown menu. Rebuilt on demand so labels always
    /// reflect live state (chosen scheme, colour name, pad connection).
    pub fn menu_rows(&self, pads_connected: [bool; MAX_PLAYERS]) -> Vec<MenuRow> {
        match self.state {
            State::Paused => {
                let mut rows = alloc::vec![MenuRow {
                    label: "RESUME".to_string(),
                    action: MenuAction::Resume,
                    swatch: None,
                }];
                // Only offered during the lull, and never first: pausing mid-lull
                // and mashing confirm should give the breather back, not cut it.
                if self.waves.between_waves() {
                    rows.push(MenuRow {
                        label: "START WAVE".to_string(),
                        action: MenuAction::StartWave,
                        swatch: None,
                    });
                }
                rows.push(MenuRow {
                    label: "EXIT".to_string(),
                    action: MenuAction::AskAbandon,
                    swatch: None,
                });
                rows
            }
            // "No" sits first, so a mashed confirm button keeps the run.
            State::ConfirmAbandon => vec![
                MenuRow { label: "NO".to_string(), action: MenuAction::KeepPlaying, swatch: None },
                MenuRow { label: "YES".to_string(), action: MenuAction::AbandonRun, swatch: None },
            ],
            State::DevMenu => vec![
                MenuRow {
                    label: format!("WAVE: {}", self.dev.wave),
                    action: MenuAction::AdjustDevWave,
                    swatch: None,
                },
                MenuRow {
                    label: format!("SCORE: {}", self.dev.score),
                    action: MenuAction::AdjustDevScore,
                    swatch: None,
                },
                MenuRow {
                    label: format!("KIND: {}", self.dev.kind_label()),
                    action: MenuAction::AdjustDevKind,
                    swatch: None,
                },
                MenuRow {
                    label: format!("RULE: {}", self.dev.rule_label()),
                    action: MenuAction::AdjustDevRule,
                    swatch: None,
                },
                MenuRow {
                    label: format!("PLAYERS: {}", self.dev.players.min(self.max_players)),
                    action: MenuAction::AdjustDevPlayers,
                    swatch: None,
                },
                MenuRow {
                    label: format!("ATTACK: {}", self.dev.attack.label()),
                    action: MenuAction::AdjustDevAttack,
                    swatch: None,
                },
                MenuRow {
                    label: format!("ATK LEVEL: {}", self.dev.attack_level),
                    action: MenuAction::AdjustDevAttackLevel,
                    swatch: None,
                },
                // The whole set on one row, then each on its own. Reaching a
                // boon in a real run costs tens of thousands of points, and
                // most of what wants testing wants all of them at once.
                MenuRow {
                    label: format!("ALL UPGRADES: {}", Self::on_off(self.dev.all_boons())),
                    action: MenuAction::AdjustDevAllBoons,
                    swatch: None,
                },
                MenuRow {
                    label: format!("DOUBLE JUMP: {}", Self::on_off(self.dev.boons.double_jump)),
                    action: MenuAction::AdjustDevDoubleJump,
                    swatch: None,
                },
                MenuRow {
                    label: format!("FREE DASH: {}", Self::on_off(self.dev.boons.dash_free)),
                    action: MenuAction::AdjustDevDashFree,
                    swatch: None,
                },
                MenuRow {
                    label: format!("SHIELD: {}", Self::on_off(self.dev.boons.shield)),
                    action: MenuAction::AdjustDevShield,
                    swatch: None,
                },
                MenuRow {
                    label: format!("WALL: {}", self.dev.wall_label()),
                    action: MenuAction::AdjustDevWall,
                    swatch: None,
                },
                MenuRow {
                    label: "START".to_string(),
                    action: MenuAction::StartDevRun,
                    swatch: None,
                },
                MenuRow {
                    label: "BACK".to_string(),
                    action: MenuAction::Back,
                    swatch: None,
                },
            ],
            State::Settings => {
                let mut rows = Vec::new();
                for i in 0..self.max_players {
                    let cfg = self.settings.players[i];
                    let scheme = &self.schemes[cfg.scheme.min(self.schemes.len() - 1)];
                    let suffix = if scheme.is_gamepad {
                        if pads_connected
                            .get(scheme.pad_index)
                            .copied()
                            .unwrap_or(false)
                        {
                            " *"
                        } else {
                            " (OFF)"
                        }
                    } else {
                        ""
                    };
                    rows.push(MenuRow {
                        label: format!("P{}: {}{}", i + 1, scheme.label, suffix),
                        action: MenuAction::AdjustScheme(i),
                        swatch: None,
                    });
                    rows.push(MenuRow {
                        label: format!("P{} COLOR: {}", i + 1, PLAYER_COLORS[cfg.color_index].0),
                        action: MenuAction::AdjustColor(i),
                        swatch: Some(cfg.color_index),
                    });
                }
                rows.push(MenuRow {
                    label: format!("MUSIC: {}", self.settings.music_volume),
                    action: MenuAction::AdjustVolume(VolumeChannel::Music),
                    swatch: None,
                });
                rows.push(MenuRow {
                    label: format!("EFFECTS: {}", self.settings.sfx_volume),
                    action: MenuAction::AdjustVolume(VolumeChannel::Sfx),
                    swatch: None,
                });
                rows.push(MenuRow {
                    label: "BACK".to_string(),
                    action: MenuAction::Back,
                    swatch: None,
                });
                rows
            }
            _ => {
                let mut rows = vec![MenuRow {
                    label: "1 PLAYER".to_string(),
                    action: MenuAction::StartRun(1),
                    swatch: None,
                }];
                if self.max_players >= 2 {
                    rows.push(MenuRow {
                        label: "2 PLAYERS".to_string(),
                        action: MenuAction::StartRun(2),
                        swatch: None,
                    });
                }
                rows.push(MenuRow {
                    label: "SETTINGS".to_string(),
                    action: MenuAction::OpenSettings,
                    swatch: None,
                });
                rows.push(MenuRow {
                    label: "EXIT".to_string(),
                    action: MenuAction::Quit,
                    swatch: None,
                });
                rows
            }
        }
    }

    fn current_menu_mut(&mut self) -> &mut Menu {
        match self.state {
            State::Settings => &mut self.settings_menu,
            State::DevMenu => &mut self.dev_menu,
            State::Paused => &mut self.pause_menu,
            State::ConfirmAbandon => &mut self.confirm_menu,
            _ => &mut self.title_menu,
        }
    }

    fn menu_move(&mut self, delta: i32) {
        let count = self.menu_rows([false; MAX_PLAYERS]).len();
        self.current_menu_mut().move_by(delta, count);
    }

    fn selected_action(&self) -> Option<MenuAction> {
        let menu = match self.state {
            State::Settings => &self.settings_menu,
            State::DevMenu => &self.dev_menu,
            State::Paused => &self.pause_menu,
            State::ConfirmAbandon => &self.confirm_menu,
            _ => &self.title_menu,
        };
        let rows = self.menu_rows([false; MAX_PLAYERS]);
        rows.get(menu.index).map(|r| r.action)
    }

    fn menu_adjust(&mut self, dir: i32) {
        match self.selected_action() {
            Some(MenuAction::AdjustScheme(i)) => self.settings.cycle(i, true, dir, self.schemes.len()),
            Some(MenuAction::AdjustColor(i)) => self.settings.cycle(i, false, dir, self.schemes.len()),
            Some(MenuAction::AdjustVolume(ch)) => self.settings.adjust_volume(ch, dir),
            Some(MenuAction::AdjustDevWave) => self.dev.adjust_wave(dir),
            Some(MenuAction::AdjustDevScore) => self.dev.adjust_score(dir),
            Some(MenuAction::AdjustDevKind) => self.dev.cycle_kind(dir),
            Some(MenuAction::AdjustDevRule) => self.dev.cycle_rule(dir),
            Some(MenuAction::AdjustDevPlayers) => self.dev.adjust_players(dir, self.max_players),
            Some(MenuAction::AdjustDevAttack) => self.dev.cycle_attack(dir),
            Some(MenuAction::AdjustDevAttackLevel) => self.dev.adjust_attack_level(dir),
            // Left and right do the same thing on a switch; there is no
            // direction to a yes or a no.
            Some(MenuAction::AdjustDevAllBoons) => self.dev.toggle_all_boons(),
            Some(MenuAction::AdjustDevDoubleJump) => self.dev.toggle_double_jump(),
            Some(MenuAction::AdjustDevDashFree) => self.dev.toggle_dash_free(),
            Some(MenuAction::AdjustDevShield) => self.dev.toggle_shield(),
            Some(MenuAction::AdjustDevWall) => self.dev.cycle_wall(dir),
            _ => {}
        }
    }

    fn menu_confirm(&mut self) {
        match self.selected_action() {
            Some(MenuAction::StartRun(n)) => self.start_run(n),
            Some(MenuAction::OpenSettings) => {
                self.state = State::Settings;
                self.settings_menu.index = 0;
            }
            Some(MenuAction::Back) => {
                self.state = State::Title;
                self.title_menu.index = 0;
            }
            // Adjustable rows step forward when confirmed.
            Some(MenuAction::AdjustScheme(i)) => self.settings.cycle(i, true, 1, self.schemes.len()),
            Some(MenuAction::AdjustColor(i)) => self.settings.cycle(i, false, 1, self.schemes.len()),
            Some(MenuAction::AdjustVolume(ch)) => self.settings.adjust_volume(ch, 1),
            Some(MenuAction::AdjustDevWave) => self.dev.adjust_wave(1),
            Some(MenuAction::AdjustDevScore) => self.dev.adjust_score(1),
            Some(MenuAction::AdjustDevKind) => self.dev.cycle_kind(1),
            Some(MenuAction::AdjustDevRule) => self.dev.cycle_rule(1),
            Some(MenuAction::AdjustDevPlayers) => self.dev.adjust_players(1, self.max_players),
            Some(MenuAction::AdjustDevAttack) => self.dev.cycle_attack(1),
            Some(MenuAction::AdjustDevAttackLevel) => self.dev.adjust_attack_level(1),
            Some(MenuAction::AdjustDevAllBoons) => self.dev.toggle_all_boons(),
            Some(MenuAction::AdjustDevDoubleJump) => self.dev.toggle_double_jump(),
            Some(MenuAction::AdjustDevDashFree) => self.dev.toggle_dash_free(),
            Some(MenuAction::AdjustDevShield) => self.dev.toggle_shield(),
            Some(MenuAction::AdjustDevWall) => self.dev.cycle_wall(1),
            Some(MenuAction::StartDevRun) => self.start_dev_run(),
            Some(MenuAction::Quit) => self.quit_requested = true,
            Some(MenuAction::Resume) => self.toggle_pause(),
            Some(MenuAction::StartWave) => {
                self.waves.skip_countdown();
                // Straight back into it: nobody asks for the wave and then
                // wants to keep looking at the menu.
                self.toggle_pause();
            }
            Some(MenuAction::AskAbandon) => {
                self.state = State::ConfirmAbandon;
                self.confirm_menu.index = 0;
            }
            Some(MenuAction::KeepPlaying) => {
                self.state = State::Paused;
                self.pause_menu.index = 0;
            }
            Some(MenuAction::AbandonRun) => {
                // Same road as dying, minus the result screen: the score was
                // earned, so it still counts towards the record.
                self.game_over();
                self.finish_game_over();
            }
            None => {}
        }
    }

    fn apply_player_input(&mut self, input: &InputFrame) {
        let v = self.viewport;
        for i in 0..self.players.len() {
            if self.players[i].dead {
                continue;
            }
            let mut intent = input.players[i];
            // While the choice is up there is nothing to jump over and nothing
            // to wall off. Attack stays: it is how the choice is made.
            if self.offer.is_some() {
                intent.jump = false;
                intent.slam = false;
            }

            // Movement is held state, so it is simply mirrored each tick.
            if intent.left && !intent.right {
                self.players[i].ax = -v.wper(PLAYER_MOVE_PCT);
                self.players[i].facing_right = false;
            } else if intent.right && !intent.left {
                self.players[i].ax = v.wper(PLAYER_MOVE_PCT);
                self.players[i].facing_right = true;
            } else {
                self.players[i].ax = 0.0;
            }

            let rule = self.waves.rule;
            if intent.jump && rule != WaveRule::NoJumps {
                // A jump off nothing is the same jump, paid for out of a stock
                // that the next landing refills. The wave rule still decides
                // first: a boon does not buy back a system the wave turned off.
                let airborne = !self.players[i].grounded;
                let can_jump = if airborne {
                    let left = self.players[i].air_jumps;
                    if left > 0 {
                        self.players[i].air_jumps = left - 1;
                        true
                    } else {
                        false
                    }
                } else {
                    true
                };
                if can_jump {
                    self.players[i].ay = -v.hper(2.0);
                    self.audio.push(AudioEvent::Jump);
                }
            }
            if intent.slam {
                match rule {
                    // Grounded: there is no arc to slam out of, so the wall goes
                    // up on the spot instead.
                    WaveRule::NoJumps if self.players[i].grounded => {
                        if self.players[i].super_charges > 0 {
                            self.players[i].super_charges -= 1;
                            self.players[i].attacks_since_power_up += 1;
                            let body = self.players[i].body;
                            self.players[i].field.activate(&body, &v);
                            self.wall_shove(i);
                            // A wall is one action, and an enemy may only be caught by it once.
                            self.players[i].strike_id = self.players[i].strike_id.wrapping_add(1).max(1);
                            self.audio.push(AudioEvent::Slam);
                        }
                    }
                    // No wall this wave: the same button buys a moment of being
                    // untouchable, paid for the same way.
                    WaveRule::NoWall if !self.players[i].grounded => {
                        if self.players[i].super_charges > 0 && !self.players[i].invulnerable {
                            self.players[i].super_charges -= 1;
                            self.players[i].attacks_since_power_up += 1;
                            self.players[i].invulnerable = true;
                        }
                        self.players[i].ay = v.hper(5.0);
                    }
                    WaveRule::NoWall => {}
                    // Airborne, on every rule that still has a wall - the one
                    // with no jumping included. Being up there was not the
                    // player's choice on that wave, a hit put them there, and
                    // down used to have nothing to do about it. Now it does the
                    // ordinary thing: hurries the fall, and covers it, so the
                    // landing that raises the wall cannot itself be punished.
                    _ if !self.players[i].grounded => {
                        self.players[i].field.readiness = true;
                        self.players[i].ay = v.hper(5.0);
                    }
                    _ => {}
                }
            }
            // Committed once it starts: no steering, no second dash out of the
            // first, and the direction is whichever way the player is facing.
            if intent.dash
                && !self.players[i].dashing()
                && (self.players[i].dash_cooldown == 0 || self.players[i].boons.dash_free)
                && self.pay_for_dash(i)
            {
                self.players[i].dash_ticks = DASH_TICKS;
                self.players[i].dash_dir = if self.players[i].facing_right { 1.0 } else { -1.0 };
            }
            // A thrown attack keeps going while the button is down; everything
            // else takes one swing per press.
            let repeat = self.players[i].attack == AttackKind::Bullet && intent.attack_held;
            if (intent.attack || repeat) && !self.players[i].attacking() {
                let timer = self.timer;
                let p = &mut self.players[i];
                if p.combo == 0 || timer - p.combo_timestamp > COMBO_WINDOW_TICKS {
                    p.combo_timestamp = timer;
                    p.combo = 1;
                } else if p.combo < 2 {
                    p.combo += 1;
                } else {
                    p.combo = 0;
                }
                p.attack_ticks = p.attack.swing_ticks(ATTACK_TICKS);
                // A fresh number for the swing. Enemies record it, which is how
                // a kind that may only land once per enemy tells one swing from
                // the next. Wrapping is fine: it would take eight hundred hours
                // of swinging, and a collision only costs one skipped hit.
                self.audio.push(AudioEvent::Hit);
                self.players[i].strike_id = self.take_strike();

                // A kind that throws or places holds nothing out; it puts
                // something into the world and that thing does the work. How
                // many may be out at once is what its level buys. The two
                // differ on what happens at the cap: a shot already in the air
                // is not recalled to make room, while a placed box is.
                let (kind, level) = (self.players[i].attack, self.players[i].attack_level);
                let damage = kind.damage(level);
                let reach = self.waves.rule.gun_reach();
                let wave = self.wave;
                match kind {
                    AttackKind::Bullet => {
                        let out = self.bullets.iter().filter(|b| b.owner == i).count();
                        if out < kind.max_instances(level) {
                            let body = self.players[i].body;
                            let facing = self.players[i].facing_right;
                            let push = kind.knockback_scale();
                            let lift = kind.lift_scale();
                            self.bullets
                                .push(Bullet::new(i, &body, facing, &v, damage, push, lift));
                        }
                    }
                    AttackKind::Frozen => {
                        // A press always places. Once this player's own are at
                        // the cap their level buys, the oldest makes room for
                        // it. Refusing instead left the button dead in the hand
                        // while boxes the player had walked away from stood
                        // somewhere behind them.
                        let cap = kind.max_instances(level).max(1);
                        while self.traps.iter().filter(|t| t.owner == i).count() >= cap {
                            // They are pushed in order, so the first belonging
                            // to this player is the oldest of theirs.
                            let Some(oldest) = self.traps.iter().position(|t| t.owner == i) else {
                                break;
                            };
                            self.traps.remove(oldest);
                        }
                        let body = self.players[i].projected_swing(
                            &v,
                            reach,
                            wave,
                            kind.length_scale(level, 1.0),
                        );
                        self.traps.push(Trap {
                            body,
                            owner: i,
                            damage,
                            knockback: kind.knockback_scale(),
                            lift: kind.lift_scale(),
                            ticks_left: TRAP_LIFE_TICKS,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    fn update_presentation(&mut self) {
        let v = self.viewport;
        for i in 0..self.players.len() {
            if self.players[i].dead {
                continue;
            }
            let reach = self.waves.rule.gun_reach();
            let wave = self.wave;
            self.players[i].update_gun(&v, reach, wave);
            if self.players[i].field.active {
                let body = self.players[i].body;
                self.players[i].field.grow(&v, &body);
            }
        }
    }

    fn update_simulation(&mut self) {
        // Sights last exactly one tick; whoever is still aiming re-adds theirs.
        self.aim_dots.clear();

        for i in 0..self.players.len() {
            if self.players[i].dead {
                continue;
            }
            if self.update_player(i) {
                return; // the run ended mid-update
            }
        }
        // The camera settles before anything else is placed, so enemies that
        // clamp or spawn against the view all see the same, final edges. Doing
        // it at the end of the tick left flyers a frame behind the scroll.
        self.update_camera();

        if self.update_zombies() {
            return;
        }
        if self.update_flyers() {
            return;
        }
        self.update_detached();
        self.update_offer();
        self.update_explosions();
        self.popups.retain(|p| !p.is_expired(self.timer));
        if self.update_projectiles() {
            return;
        }
        self.recycle_stragglers();
        self.advance_waves();
        self.timer += 1;
        for p in self.players.iter_mut() {
            if p.attack_ticks > 0 {
                p.attack_ticks -= 1;
            }
        }
    }

    /// A rolled combination at boss health, named on arrival like any other.
    fn spawn_rolled_boss(&mut self) {
        let v = self.viewport;
        // Forced first, so the name below is the name of what arrives.
        let recipe = Recipe::roll(self.wave, &mut self.elite_rng).as_boss();
        let mut z = recipe.build_boss(&v, self.wave, self.timer, &mut self.elite_rng);
        z.body.x += self.camera_x;
        self.elite_notice = Some((recipe.label(), self.timer));
        self.zombies.push(z);
    }

    fn spawn_flying_boss(&mut self) {
        let v = self.viewport;
        let size_ref = self.players[0].body;
        let timer = self.timer;
        let mut f = Flyer::flying_boss(&v, &size_ref, timer, &mut self.rng);
        f.body.x += self.camera_x;
        self.flyers.push(f);
    }

    fn spawn_shedder_boss(&mut self) {
        let v = self.viewport;
        let mut boss = Zombie::shedder_boss(&v, &mut self.rng);
        boss.body.x += self.camera_x;
        self.zombies.push(boss);
    }

    fn advance_waves(&mut self) {
        let live = self.zombies.len() + self.flyers.len();
        let action = self
            .waves
            .update(self.wave, self.spawn_count, live, &mut self.rng);
        let v = self.viewport;

        match action {
            WaveAction::Idle => {}
            WaveAction::SpawnBosses(count) => {
                self.background.set_target(BG_FIGHT);
                for _ in 0..count {
                    self.spawn_count += 1;
                    let mut boss = Zombie::boss(&v, self.wave, &mut self.rng);
                    // Constructors place enemies relative to the view; this is
                    // where a view-relative x becomes a world one.
                    boss.body.x += self.camera_x;
                    self.zombies.push(boss);
                }
            }
            WaveAction::SpawnGround(kind) => {
                self.background.set_target(BG_FIGHT);
                self.spawn_count += 1;
                let plain = self.plain_enemy_color();
                let z = match kind {
                    GroundKind::Base => Zombie::from_edge(&v, &mut self.rng, plain),
                    GroundKind::Runt => Zombie::runt(&v, &mut self.rng),
                    GroundKind::Jumper => Zombie::jumper(&v, &mut self.rng),
                    GroundKind::Leaper => Zombie::leaper(&v, &mut self.rng),
                    GroundKind::Armored => Zombie::armored(&v, &mut self.rng),
                    GroundKind::Frenzied => Zombie::frenzied(&v, &mut self.rng),
                    GroundKind::Splitter => Zombie::splitter(&v, &mut self.rng),
                    GroundKind::Blinker => Zombie::blinker(&v, &mut self.rng),
                    GroundKind::Shooter => Zombie::shooter(&v, &mut self.rng),
                    GroundKind::Shedder => Zombie::shedder(&v, &mut self.rng),
                };
                let mut z = z;
                z.body.x += self.camera_x;
                self.zombies.push(z);
            }
            WaveAction::SpawnFlyer(kind) => {
                self.background.set_target(BG_FIGHT);
                self.spawn_count += 1;
                let size_ref = self.players[0].body;
                let timer = self.timer;
                let f = match kind {
                    FlyerKind::Base => Flyer::from_edge(&v, &size_ref, timer, &mut self.rng),
                    FlyerKind::Teleporter => Flyer::teleporter(&v, &size_ref, timer, &mut self.rng),
                };
                let mut f = f;
                f.body.x += self.camera_x;
                self.flyers.push(f);
            }
            WaveAction::SpawnShedderBoss => {
                self.background.set_target(BG_FIGHT);
                self.spawn_count += 1;
                self.spawn_shedder_boss();
            }
            WaveAction::SpawnFlyingBoss => {
                self.background.set_target(BG_FIGHT);
                self.spawn_count += 1;
                self.spawn_flying_boss();
            }
            WaveAction::SpawnElite(count) => {
                self.background.set_target(BG_FIGHT);
                // Each one replaces an ordinary enemy rather than adding to the
                // wave: past the ramp the wave is the same size and made of
                // worse things, which is the whole shape of the late game.
                for _ in 0..count {
                    self.spawn_count += 1;
                    let recipe = Recipe::roll(self.wave, &mut self.elite_rng);
                    let mut z = recipe.build(&v, self.wave, self.timer, &mut self.elite_rng);
                    z.body.x += self.camera_x;
                    // Named on arrival and only then. The halo says "not off
                    // the list"; this says which one of the sixty-four it is,
                    // once, while there is still time to act on knowing. With
                    // several at once the last one named is the one that shows,
                    // which is the honest summary of a pair.
                    self.elite_notice = Some((recipe.label(), self.timer));
                    self.zombies.push(z);
                }
            }
            WaveAction::SpawnRolledBoss => {
                self.background.set_target(BG_FIGHT);
                self.spawn_count += 1;
                self.spawn_rolled_boss();
            }
            WaveAction::SpawnBossGroup(kinds) => {
                self.background.set_target(BG_FIGHT);
                for kind in kinds {
                    self.spawn_count += 1;
                    match kind {
                        BossKind::Ground => {
                            let mut boss = Zombie::boss(&v, self.wave, &mut self.rng);
                            boss.body.x += self.camera_x;
                            self.zombies.push(boss);
                        }
                        BossKind::Flying => self.spawn_flying_boss(),
                        BossKind::Shedder => self.spawn_shedder_boss(),
                        BossKind::Rolled => self.spawn_rolled_boss(),
                    }
                }
            }
            WaveAction::ClearWave => self.on_wave_cleared(),
        }
    }

    /* ---------------- player ---------------- */

    /// Returns true when the run ended.
    fn update_player(&mut self, i: usize) -> bool {
        let v = self.viewport;
        let gravity = self.gravity;
        let timer = self.timer;

        if self.players[i].ay < gravity {
            self.players[i].ay += v.hper(0.1);
        }

        let (y, ay, h) = (
            self.players[i].body.y,
            self.players[i].ay,
            self.players[i].body.h,
        );
        if y + ay + h > v.hper(GROUND_Y_PCT) {
            self.players[i].body.y = v.hper(GROUND_Y_PCT) - h;
            // Touching down ends the throw. Without this the player keeps
            // drifting along the floor after a hit.
            self.players[i].knockback_x = 0.0;
            if self.players[i].invulnerable {
                // The trade for a no-wall wave: land, and get a quarter back.
                self.players[i].invulnerable = false;
                let hpmax = self.players[i].hpmax;
                self.players[i].hp = (self.players[i].hp + hpmax * 0.25).min(hpmax);
            }
            self.players[i].grounded = true;
            self.players[i].air_jumps = if self.players[i].boons.double_jump { 1 } else { 0 };
            if self.players[i].field.readiness && self.players[i].super_charges > 0 {
                self.players[i].attacks_since_power_up += 1;
                self.players[i].super_charges -= 1;
                let body = self.players[i].body;
                self.players[i].field.activate(&body, &v);
                self.wall_shove(i);
                // A wall is one action, and an enemy may only be caught by it once.
                self.players[i].strike_id = self.players[i].strike_id.wrapping_add(1).max(1);
                self.audio.push(AudioEvent::Slam);
            }
            self.players[i].field.readiness = false;
        } else {
            for e in 0..self.explosions.len() {
                if self.players[i].body.intersects(&self.explosions[e].body) {
                    self.players[i].ay = -v.hper(2.0);
                }
            }
            self.players[i].body.y += self.players[i].ay;
            self.players[i].grounded = false;
        }

        // Walking and being thrown are separate velocities: `ax` is rebuilt
        // from the pad every tick, so knockback has to live on its own.
        // The field has no ends, so nothing stops horizontal travel. What keeps
        // a co-op pair together is the leash in `update_camera`, not a wall.
        //
        // A dash is a third channel, and it overrides the other two rather than
        // adding to them: holding back during one should not shorten it. It
        // cannot ride on `knockback_x` either, because that is wiped on every
        // tick spent touching the ground.
        let dx = if self.players[i].dashing() {
            self.players[i].dash_ticks -= 1;
            if self.players[i].dash_ticks == 0 {
                // Left at zero when the dash is free, so the indicator does not
                // spend a second saying "wait" about something already
                // available. Energy is the only gate then.
                if !self.players[i].boons.dash_free {
                    self.players[i].dash_cooldown = DASH_COOLDOWN_TICKS;
                }
                // The shove and its immunity outlast the travel. `timer` only
                // advances at the end of the tick, so this deadline covers the
                // rest of this frame and then exactly DASH_GRACE_TICKS more.
                self.players[i].dash_grace_until = timer + DASH_GRACE_TICKS;
            }
            self.players[i].dash_dir * v.wper(DASH_DISTANCE_PCT) / DASH_TICKS as f32
        } else {
            self.players[i].ax + self.players[i].knockback_x
        };
        self.players[i].body.x += dx;
        self.players[i].knockback_x *= KNOCKBACK_DECAY;
        if libm::fabsf(self.players[i].knockback_x) < 0.01 {
            self.players[i].knockback_x = 0.0;
        }
        if self.players[i].dash_cooldown > 0 {
            self.players[i].dash_cooldown -= 1;
        }

        let needed = self.players[i].energy_needed(self.wave);
        if self.players[i].energy >= needed {
            let p = &mut self.players[i];
            p.energy -= needed;
            p.attacks_since_power_up = 0;
            p.super_charges += 1;
            p.hp = (p.hp + 10.0).min(255.0);
            p.score += 2;
        }
        false
    }

    /// The living player whose melee box or ultimate field is touching `body`.
    fn find_attacker(&self, body: &Body) -> Option<(usize, HitSource)> {
        for (i, p) in self.players.iter().enumerate() {
            if p.dead {
                continue;
            }
            if p.field.active && body.intersects(&p.field.body) {
                return Some((i, HitSource::Field));
            }
            // Both boxes of a two-sided swing count, and they are the same
            // swing: an enemy caught by one is not caught again by the other.
            if p.attacking() && !p.attack.detached() && p.strike_boxes().any(|g| body.intersects(g))
            {
                return Some((i, HitSource::Swing));
            }
        }
        // Thrown and placed attacks are checked after the held ones: a player
        // standing in their own trap should still be swinging, not stuck
        // re-triggering it.
        for (bi, b) in self.bullets.iter().enumerate() {
            if !b.dead && body.intersects(&b.body) {
                return Some((b.owner, HitSource::Bullet(bi)));
            }
        }
        for (ti, t) in self.traps.iter().enumerate() {
            if t.ticks_left > 0 && body.intersects(&t.body) {
                return Some((t.owner, HitSource::Trap(ti)));
            }
        }
        None
    }

    /* ---------------- zombies ---------------- */

    fn update_zombies(&mut self) -> bool {
        let v = self.viewport;
        let gravity = self.gravity;
        let timer = self.timer;
        let team_score = self.total_score();
        let ground = v.hper(GROUND_Y_PCT);

        let mut i = self.zombies.len();
        while i > 0 {
            i -= 1;
            if i >= self.zombies.len() {
                continue;
            }

            self.tick_zombie_behavior(i);

            let Some(target) = self.nearest_player(self.zombies[i].body.x) else {
                return true;
            };
            let target_x = self.players[target].body.x;

            if let Some((attacker, source)) = self.find_attacker(&self.zombies[i].body) {
                let by_field = source == HitSource::Field;
                if by_field {
                    // Once per wall, not once per tick. The field stands for
                    // about seventeen ticks while it grows, and it used to set
                    // health to -1 outright, which is the same however many
                    // times it lands - so nothing ever noticed it landing over
                    // and over. A number does notice: seventeen touches of 300
                    // is five thousand.
                    let wall = self.players[attacker].strike_id;
                    if self.zombies[i].last_strike != wall {
                        self.zombies[i].last_strike = wall;
                        if self.zombies[i].is_boss {
                            self.zombies[i].hp -= self.zombies[i].hpmax * FIELD_BOSS_FRACTION;
                        } else {
                            self.zombies[i].hp -= FIELD_DAMAGE;
                        }
                    }
                }
                let first_hit = !self.zombies[i].hurt_once;
                self.zombies[i].hurt_once = true;
                if first_hit
                    && self.zombies[i].hp >= 0.0
                    && self.zombies[i].behaviors.blink
                {
                    self.blink_away(i);
                    continue;
                }
                if self.zombies[i].hp < 0.0 {
                    // Boss payout is keyed off an explicit flag: runts and
                    // splitter children carry their own hpmax and must not be
                    // mistaken for bosses.
                    let (gain, text) = if self.zombies[i].is_boss {
                        let reward = 100 * self.wave;
                        (reward, format!("+{}", reward))
                    } else {
                        let reward = self.zombies[i].reward;
                        (reward, format!("+{}", reward))
                    };
                    self.players[attacker].score += gain;
                    self.award_energy(attacker, gain);
                    self.players[attacker].kills += 1;
                    let (zx, zy, zw, zh) = (
                        self.zombies[i].body.x,
                        self.zombies[i].body.y,
                        self.zombies[i].body.w,
                        self.zombies[i].body.h,
                    );
                    self.popups.push(ScorePopup {
                        x: zx,
                        y: zy,
                        text,
                        created_at: self.timer,
                    });
                    // Every death leaves a blast. Ordinarily it is scenery -
                    // but a kill that finished a combo with the plain attack
                    // leaves one that bites, which is what keeping the rhythm
                    // is worth and what every upgrade trades away.
                    let finisher = self.players[attacker].attack == AttackKind::Basic
                        && self.players[attacker].combo == 2;
                    let blast = if finisher {
                        let strike = self.take_strike();
                        Explosion::lethal(zx + zw, zy + zh, &v, COMBO_BLAST_DAMAGE, strike)
                    } else {
                        Explosion::new(zx + zw, zy + zh, &v)
                    };
                    self.explosions.push(blast);
                    for _ in 0..self.zombies[i].splits_into {
                        let child = Zombie::child(&self.zombies[i].clone(), &v, &mut self.rng);
                        self.zombies.push(child);
                    }
                    self.zombies.remove(i);
                    continue;
                }
                let kind = self.players[attacker].attack;
                let level = self.players[attacker].attack_level;
                // A kind that only lands once per swing checks the enemy's
                // memory of which swing last reached it. Indices into the enemy
                // vectors are not stable - enemies are removed mid-loop - so
                // the identity lives on the swing, not on a list of victims.
                let strike = self.players[attacker].strike_id;
                let already = self.zombies[i].last_strike == strike && strike != 0;
                // Damage and throw come from whatever actually reached the
                // enemy. A thrown or placed attack carries the numbers it was
                // made with, so a later upgrade cannot change a shot already in
                // the air.
                let (hit_damage, push, lift) = match source {
                    HitSource::Field => (0.0, 1.0, 1.0),
                    HitSource::Swing => (
                        kind.damage(level),
                        kind.knockback_scale(),
                        kind.lift_scale(),
                    ),
                    HitSource::Bullet(bi) => (
                        self.bullets[bi].damage,
                        self.bullets[bi].knockback,
                        self.bullets[bi].lift,
                    ),
                    HitSource::Trap(ti) => (
                        self.traps[ti].damage,
                        self.traps[ti].knockback,
                        self.traps[ti].lift,
                    ),
                };
                // A repeat contact from a swing that has already landed on this
                // enemy is not a hit at all: no damage, no throw, no score.
                // Paying for it would turn a kind that hits once into one that
                // earns six times over. Note this is not a `continue` - the
                // enemy still has the rest of its tick coming, gravity included.
                let repeat = source == HitSource::Swing && kind.once_per_enemy() && already;
                if !repeat {
                    // A bullet stops on the first thing it reaches.
                    if let HitSource::Bullet(bi) = source {
                        if self.bullets[bi].flying() {
                            self.bullets[bi].stop();
                        }
                    }
                self.players[attacker].score += 3;
                self.award_energy(attacker, 3);
                let (zx, zy) = (self.zombies[i].body.x, self.zombies[i].body.y);
                self.popups.push(ScorePopup {
                    x: zx,
                    y: zy,
                    text: "+3".to_string(),
                    created_at: self.timer,
                });
                let attacker_x = self.players[attacker].body.x;
                let dir = if zx > attacker_x { 1.0 } else { -1.0 };

                // A heavier swing that threw on every connecting tick would
                // clear the enemy out sooner and so land less often than a
                // light one. Those kinds hold the throw until the last tick.
                // The two halves of a throw are applied apart, because a
                // bullet wants the lift without the shove: sideways travel
                // would carry the target out of the square still working on it.
                if lift > 0.0 {
                    self.zombies[i].ay = -v.hper(2.0) * lift;
                }
                if push > 0.0 {
                    // Thrown off at exactly walking pace, whatever it is. It
                    // used to scale with the enemy's own health, which made a
                    // boss travel three times as far from a hit as a runt did.
                    self.zombies[i].ax = dir * v.wper(PLAYER_MOVE_PCT) * push;
                }
                // Only a swing does swing damage. The wall reaching an enemy
                // used to add this on top of its own, which was invisible while
                // the wall dealt a flat 255 - the 64 was what actually took an
                // ordinary enemy from exactly zero to dead - but it would make
                // a boss lose a seventh plus a hit.
                if !by_field {
                    let armor = self.zombies[i].armor;
                    self.zombies[i].hp -= hit_damage * armor;
                    self.zombies[i].last_strike = strike;
                }
                }

                // Every hit it lives through sends it out of reach, leaving a
                // husk standing where the hit landed. Checked after the damage
                // so the hit actually counts, and gated on surviving so the
                // killing blow does not send a corpse across the field.
                if self.zombies[i].behaviors.shed && self.zombies[i].hp >= 0.0 {
                    self.shed_and_flee(i);
                }

                // Answers the blow with young. Gated on `already`, so it is one
                // brood per swing and not one per connecting tick: a swing
                // covers an enemy for the better part of a dozen ticks, and
                // paying that rate would fill the field from a single hit.
                //
                // The wall is left out for free by the same gate - it stamps
                // its own identity on what it reaches before this runs - and
                // that is the right answer anyway. The ultimate exists to clear
                // a field, not to seed one.
                if self.zombies[i].broods && !already && self.zombies[i].hp >= 0.0 {
                    self.spawn_brood(i);
                }
            }

            for e in 0..self.explosions.len() {
                if self.zombies[i].body.intersects(&self.explosions[e].body) {
                    let dir = if self.zombies[i].body.x > target_x {
                        1.0
                    } else {
                        -1.0
                    };
                    self.zombies[i].ay = -v.hper(2.0);
                    self.zombies[i].ax =
                        dir * (v.wper(0.5) + v.wper(0.1) * self.zombies[i].hpmax / 255.0);
                    // A live blast bites once, not on every tick it covers the
                    // enemy while it grows - hence the identity, drawn from the
                    // same pool swings use.
                    let (blast_damage, blast_strike) =
                        (self.explosions[e].damage, self.explosions[e].strike);
                    if blast_damage > 0.0 && self.zombies[i].last_strike != blast_strike {
                        self.zombies[i].last_strike = blast_strike;
                        self.zombies[i].hp -= blast_damage;
                    }
                }
            }

            for pi in 0..self.players.len() {
                if self.players[pi].dead {
                    continue;
                }
                let dashing = self.players[pi].in_dash_window(timer);
                let p = &self.players[pi];
                if self.zombies[i].body.intersects(&p.body) && !p.field.readiness && !p.field.active
                {
                    // A dash shoves whatever it runs through up and out of the
                    // way, and neither side takes anything for it. What the
                    // player buys is room, not a kill - and while an enemy is in
                    // the air it is out of the melee box too, so the trade costs
                    // tempo as well as energy.
                    if dashing {
                        self.zombies[i].ay = -v.hper(DASH_THROW_PCT);
                        continue;
                    }
                    let source = self.zombies[i].body;
                    let ended = self.damage_player(pi, 16.0, &source, Reach::Normal);
                    if ended {
                        return true;
                    }
                    // Touching one is always enough to send it away, however
                    // many times it comes back.
                    if self.zombies[i].behaviors.blink {
                        self.blink_away(i);
                        break;
                    }
                }
            }

            // Husks hurt on contact and cannot be hurt back. They are scenery
            // with teeth rather than enemies, which is also why they never
            // count towards clearing a wave.
            for h in 0..self.zombies[i].husks.len() {
                let husk = self.zombies[i].husks[h];
                for pi in 0..self.players.len() {
                    if self.players[pi].dead {
                        continue;
                    }
                    let p = &self.players[pi];
                    if husk.intersects(&p.body) && !p.field.readiness && !p.field.active {
                        if self.damage_player(pi, HUSK_DAMAGE, &husk, Reach::ThroughDash) {
                            return true;
                        }
                    }
                }
            }

            if let Movement::Fly { offset } = self.zombies[i].movement {
                // Height is a wave rather than a fall. The same arc an ordinary
                // flyer rides, and for the same reason: it has to dip low enough
                // that a player standing on the ground can reach it, or a flying
                // enemy would simply be unkillable without jumping.
                let phase = (timer - offset) as f32 / 50.0;
                self.zombies[i].ay = 0.0;
                self.zombies[i].body.y = v.hper(FLYER_ARC_CENTER_PCT)
                    + libm::sinf(phase) * v.hper(FLYER_ARC_SWING_PCT);
            } else {
                if self.zombies[i].ay < gravity {
                    self.zombies[i].ay += v.hper(0.1);
                }
                let (zy, zay, zh) = (
                    self.zombies[i].body.y,
                    self.zombies[i].ay,
                    self.zombies[i].body.h,
                );
                if zy + zay + zh > ground {
                    self.zombies[i].body.y = ground - zh;
                } else {
                    self.zombies[i].body.y += zay;
                }
            }

            // A leaper's trajectory is its own; the chase would clamp the leap
            // speed down to walking pace and it would land short.
            if matches!(self.zombies[i].movement, Movement::Leap(_)) {
                self.zombies[i].body.x += self.zombies[i].ax;
                continue;
            }

            let mut max_ax = v.wper(0.5)
                + v.wper(
                    (libm::powf(libm::logf(team_score as f32 / 20.0 + 1.0), 2.0) * 10.0)
                        / ZOMBIE_CHASE_SPEED_RAMP,
                );
            max_ax *= self.zombies[i].speed_multiplier;
            if self.zombies[i].enrages
                && self.zombies[i].hp / self.zombies[i].hpmax < ENRAGE_HP_THRESHOLD
            {
                max_ax *= ENRAGE_SPEED_MULTIPLIER;
            }

            // Steering needs a foot on something. An enemy thrown into the air
            // keeps the momentum it had rather than swimming after the player -
            // except a flier, which is never on the ground and would otherwise
            // never accelerate at all: it would hang in its arc where it spawned
            // and simply bob.
            let on_ground =
                (self.zombies[i].body.y - (ground - self.zombies[i].body.h)).abs() < 0.001;
            let can_steer = on_ground || matches!(self.zombies[i].movement, Movement::Fly { .. });
            // Over-speed bleeds off rather than being snapped away.
            //
            // It used to snap, and while nothing ever pushed an enemy *toward*
            // the player that was invisible: knockback and blasts only ever
            // throw outward, and those decelerate through the branches below.
            // A wall that pulls does push inward, and a snap would have eaten
            // the whole impulse on the tick it was given - the pull would move
            // nothing at all while the push worked fine.
            let accel = v.wper(CHASE_ACCEL_PCT);
            if target_x - self.zombies[i].body.x > 0.0 {
                if self.zombies[i].ax > max_ax {
                    self.zombies[i].ax = (self.zombies[i].ax - accel).max(max_ax);
                } else if can_steer {
                    self.zombies[i].ax += accel;
                }
            } else if self.zombies[i].ax < -max_ax {
                self.zombies[i].ax = (self.zombies[i].ax + accel).min(-max_ax);
            } else if can_steer {
                self.zombies[i].ax -= accel;
            }

            // No horizontal limit: the field is endless and a ground enemy has
            // to be able to follow the player wherever they go. The old bounds
            // were absolute world coordinates from the days of a one-screen
            // arena, so past 1.5 screens the chase simply stopped - and once
            // straggler recycling started pulling enemies back, they bounced
            // between the player and that wall on every tick.
            self.zombies[i].body.x += self.zombies[i].ax;
        }
        false
    }

    /// Leaves a blast where a blinker stood and puts it back at the edge of the
    /// field, as if it had just walked on.
    fn blink_away(&mut self, i: usize) {
        let v = self.viewport;
        let b = self.zombies[i].body;
        self.explosions
            .push(Explosion::new(b.x + b.w, b.y + b.h, &v));
        self.zombies[i].body.x = if self.rng.flip() {
            self.camera_x - v.wper(5.0)
        } else {
            self.camera_x + v.w + v.wper(5.0)
        };
        self.zombies[i].body.y = v.hper(GROUND_Y_PCT) - self.zombies[i].body.h;
        self.zombies[i].ax = 0.0;
        self.zombies[i].ay = 0.0;
    }

    /// Leaves a husk where a shedder is standing and puts the shedder itself
    /// back beyond the player's reach.
    ///
    /// The distance is measured from the player rather than from the enemy, and
    /// it is past the melee ceiling, so no amount of ramped reach turns one hit
    /// into two. The side is a coin flip: fleeing consistently away would let
    /// the player herd it.
    /// Throws out one to [`BROOD_MAX`] young at the parent's feet.
    ///
    /// They are rolled the same way the parent was, so a brood is a handful of
    /// unrelated shapes rather than copies - and rolled at ordinary strength,
    /// because several eleven-swing enemies per blow is not a fight.
    fn spawn_brood(&mut self, i: usize) {
        let v = self.viewport;
        // Held to the same ceiling the waves are. Without this a brooder is not
        // a hard enemy, it is a losing race: every blow that fails to kill it
        // leaves the player with more to do than before.
        let room = max_concurrent_enemies(self.wave)
            .saturating_sub(self.zombies.len() + self.flyers.len());
        if room == 0 {
            return;
        }
        let want = 1 + (self.elite_rng.unit() * BROOD_MAX as f32) as usize;
        let (px, feet) = (
            self.zombies[i].body.x,
            self.zombies[i].body.y + self.zombies[i].body.h,
        );
        for _ in 0..want.min(room) {
            // Young are drawn against the same wave: a brood must not be a
            // way round the schedule its parent was held to.
            let recipe = Recipe::roll(self.wave, &mut self.elite_rng);
            let mut child = recipe.build_minion(&v, self.timer, &mut self.elite_rng);
            // Put down beside the parent rather than at the field edge: young
            // that had to walk back in would be a delay, not a consequence.
            child.body.x = px + (self.elite_rng.unit() - 0.5) * v.wper(8.0);
            child.body.y = feet - child.body.h;
            child.ay = -v.hper(1.0);
            self.zombies.push(child);
        }
    }

    fn shed_and_flee(&mut self, i: usize) {
        let v = self.viewport;
        let body = self.zombies[i].body;

        if self.zombies[i].max_husks > 0 {
            // One in, one out for an ordinary shedder. The boss's cap is never
            // reached, so it keeps every husk it has left.
            while self.zombies[i].husks.len() >= self.zombies[i].max_husks {
                self.zombies[i].husks.remove(0);
            }
            self.zombies[i].husks.push(body);
        }

        self.explosions
            .push(Explosion::new(body.x + body.w, body.y + body.h, &v));

        let from = match self.nearest_player(body.x) {
            Some(pi) => self.players[pi].body.center_x(),
            None => body.x,
        };
        let side = if self.rng.flip() { 1.0 } else { -1.0 };
        self.zombies[i].body.x = from + side * v.wper(SHEDDER_TELEPORT_PCT);
        self.zombies[i].body.y = v.hper(GROUND_Y_PCT) - self.zombies[i].body.h;
        self.zombies[i].ax = 0.0;
        self.zombies[i].ay = 0.0;
    }

    /// Drops a flyer at one edge of the view and restarts its arc there.
    fn teleport_flyer(&mut self, i: usize) {
        let v = self.viewport;
        self.flyers[i].body.x = if self.rng.flip() {
            self.camera_x + v.w - self.flyers[i].body.w
        } else {
            self.camera_x
        };
        self.flyers[i].spawn_offset = -self.timer;
        self.flyers[i].ax = v.wper(0.5);
        // Only restart the clock for something that runs on one; the boss
        // teleports from damage alone.
        if matches!(self.flyers[i].behavior, FlyerBehavior::Teleporter { .. }) {
            self.flyers[i].behavior = FlyerBehavior::Teleporter {
                cooldown: TELEPORTER_TELEPORT_EVERY,
            };
        }
    }

    /// Centre of the nearest living player, for a shooter to point at.
    fn aim_at_nearest(&self, i: usize) -> Option<(f32, f32)> {
        let target = self.nearest_player(self.zombies[i].body.center_x())?;
        let b = self.players[target].body;
        Some((b.center_x(), b.center_y()))
    }

    /// Lays the sight out as evenly spaced dots from the shooter to its mark.
    fn push_aim_dots(&mut self, i: usize, aim: (f32, f32), hot: bool) {
        let v = self.viewport;
        let from = self.zombies[i].body;
        let (sx, sy) = (from.center_x(), from.center_y());
        let (dx, dy) = (aim.0 - sx, aim.1 - sy);
        let dist = libm::sqrtf(dx * dx + dy * dy);
        if dist < 1.0 {
            return;
        }

        let spacing = v.wper(3.0).max(1.0);
        let count = ((dist / spacing) as usize).clamp(1, 24);
        let size = v.hper(1.2);
        for n in 1..=count {
            let t = n as f32 / (count + 1) as f32;
            self.aim_dots.push(AimDot {
                x: sx + dx * t - size / 2.0,
                y: sy + dy * t - size / 2.0,
                size,
                hot,
            });
        }
    }

    /// How many ticks a leap of `power_pct` spends in the air.
    ///
    /// Simulated with the same integrator the update loop uses rather than
    /// solved on paper: gravity is capped at terminal velocity, so the closed
    /// form only holds for short hops and would land a long leap short.
    fn leap_flight_ticks(v: &Viewport, power_pct: f32) -> i32 {
        let step = v.hper(0.1);
        let terminal = v.hper(1.0);
        let mut ay = -v.hper(power_pct);
        let mut y = 0.0f32;
        for t in 0..600 {
            if ay < terminal {
                ay += step;
            }
            if y + ay > 0.0 {
                return t + 1;
            }
            y += ay;
        }
        600
    }

    /// Runs everything this enemy does beyond walking.
    ///
    /// Movement is one of a kind and behaviours are a set, so the first is a
    /// match and the rest are passes. A leaping enemy can still shoot: only the
    /// ways of moving are exclusive, and only because two of them would write
    /// the same velocity on the same tick.
    fn tick_zombie_behavior(&mut self, i: usize) {
        let ground = self.viewport.hper(GROUND_Y_PCT);
        match self.zombies[i].movement {
            Movement::Run | Movement::Fly { .. } => {}
            Movement::Leap(_) => self.tick_leap(i, ground),
            Movement::Hop { .. } => self.tick_hop(i, ground),
        }
        self.tick_shot(i);
        // Blinking and shedding are answers to being hit, not things done on a
        // tick; they live at the damage site.
    }

    fn tick_leap(&mut self, i: usize, ground: f32) {
        let v = self.viewport;
        let Movement::Leap(leap) = self.zombies[i].movement else {
            return;
        };
        let z = &self.zombies[i];
        let on_ground = (z.body.y - (ground - z.body.h)).abs() < 0.001;

        if leap.airborne {
            // The arc is committed; nothing steers it now.
            if on_ground {
                self.zombies[i].ax = 0.0;
                self.zombies[i].movement = Movement::Leap(Leap {
                    crouch: LEAPER_CROUCH_TICKS,
                    airborne: false,
                });
            }
            return;
        }
        if !on_ground {
            // Knocked into the air by something else; wait for the floor.
            return;
        }

        // Winding up: dead still, which is the tell.
        self.zombies[i].ax = 0.0;
        if leap.crouch > 0 {
            self.zombies[i].movement = Movement::Leap(Leap {
                crouch: leap.crouch - 1,
                airborne: false,
            });
            return;
        }

        let Some(target) = self.nearest_player(self.zombies[i].body.center_x()) else {
            return;
        };
        let reach = v.wper(LEAPER_MAX_REACH_PCT);
        let dx =
            (self.players[target].body.center_x() - self.zombies[i].body.center_x())
                .clamp(-reach, reach);

        // Land exactly where the player is standing right now: the horizontal
        // speed is the distance divided by the time the arc will take, so the
        // two cannot disagree.
        let ticks = Self::leap_flight_ticks(&v, LEAPER_JUMP_POWER_PCT);
        self.zombies[i].ay = -v.hper(LEAPER_JUMP_POWER_PCT);
        self.zombies[i].ax = dx / ticks as f32;
        self.zombies[i].movement = Movement::Leap(Leap {
            crouch: 0,
            airborne: true,
        });
    }

    fn tick_hop(&mut self, i: usize, ground: f32) {
        let v = self.viewport;
        let Movement::Hop { cooldown } = self.zombies[i].movement else {
            return;
        };
        if cooldown > 0 {
            self.zombies[i].movement = Movement::Hop {
                cooldown: cooldown - 1,
            };
        } else if (self.zombies[i].body.y - (ground - self.zombies[i].body.h)).abs() < 0.001 {
            // Gravity and the ground clamp carry the rest of the arc.
            self.zombies[i].ay = -v.hper(JUMPER_JUMP_POWER_PCT);
            self.zombies[i].movement = Movement::Hop {
                cooldown: JUMPER_JUMP_EVERY,
            };
        }
    }

    fn tick_shot(&mut self, i: usize) {
        let v = self.viewport;
        let Some(cooldown) = self.zombies[i].behaviors.shoot else {
            return;
        };
        if cooldown > 0 {
            self.zombies[i].behaviors.shoot = Some(cooldown - 1);
            // Show the sight for the run-up to the shot. While it is white it
            // follows the player; once it goes red the aim is locked and moving
            // out of the line actually works.
            if cooldown <= SHOOTER_AIM_TICKS {
                let locked = cooldown <= SHOOTER_LOCK_TICKS;
                // Track while white. Locked with nothing to lock onto can only
                // happen to a shooter that came by its cooldown some other way,
                // but it should still have a sight.
                if !locked || self.zombies[i].aim.is_none() {
                    self.zombies[i].aim = self.aim_at_nearest(i);
                }
                if let Some(aim) = self.zombies[i].aim {
                    self.push_aim_dots(i, aim, locked);
                }
            }
            return;
        }
        let Some(target) = self.nearest_player(self.zombies[i].body.x) else {
            return;
        };
        self.zombies[i].behaviors.shoot = Some(SHOOTER_FIRE_EVERY);
        // Fire down the locked sight, not at wherever the player got to.
        let from = self.zombies[i].body;
        let aim = self.zombies[i].aim;
        self.zombies[i].aim = None;
        let (dx, dy) = match aim {
            Some((ax, ay)) => (ax - from.center_x(), ay - from.center_y()),
            None => {
                let to = self.players[target].body;
                (to.center_x() - from.center_x(), to.center_y() - from.center_y())
            }
        };
        let dist = libm::sqrtf(dx * dx + dy * dy).max(1.0);
        let speed = v.wper(1.2);
        self.projectiles.push(Projectile {
            body: Body::new(from.center_x(), from.center_y(), v.wper(1.5), v.hper(1.5)),
            ax: dx / dist * speed,
            ay: dy / dist * speed,
            damage: SHOOTER_PROJECTILE_DAMAGE,
            dead: false,
        });
    }

    /* ---------------- flyers ---------------- */

    fn update_flyers(&mut self) -> bool {
        let v = self.viewport;
        let timer = self.timer;
        let mut i = self.flyers.len();
        while i > 0 {
            i -= 1;
            if i >= self.flyers.len() {
                continue;
            }

            if let FlyerBehavior::Teleporter { cooldown } = self.flyers[i].behavior {
                if cooldown > 0 {
                    self.flyers[i].behavior = FlyerBehavior::Teleporter {
                        cooldown: cooldown - 1,
                    };
                } else {
                    self.flyers[i].behavior = FlyerBehavior::Teleporter {
                        cooldown: TELEPORTER_TELEPORT_EVERY,
                    };
                    self.teleport_flyer(i);
                }
            }

            if self.nearest_player(self.flyers[i].body.x).is_none() {
                return true;
            }

            if let Some((attacker, source)) = self.find_attacker(&self.flyers[i].body) {
                let by_field = source == HitSource::Field;
                let (fx, fy) = (self.flyers[i].body.x, self.flyers[i].body.y);
                // The wall takes a share off a boss and kills anything else
                // outright, exactly as it does on the ground. It used to be an
                // instant kill here whatever the health, which meant one charge
                // ended the flying boss while forty walls could not have ended
                // a ground one.
                if by_field {
                    // Once per wall, as on the ground.
                    let wall = self.players[attacker].strike_id;
                    if self.flyers[i].last_strike != wall {
                        self.flyers[i].last_strike = wall;
                        if self.flyers[i].is_boss {
                            self.flyers[i].hp -= self.flyers[i].hpmax * FIELD_BOSS_FRACTION;
                        } else {
                            self.flyers[i].hp -= FIELD_DAMAGE;
                        }
                    }
                }
                if self.flyers[i].hp < 0.0 {
                    let (gain, text) = if self.flyers[i].is_boss {
                        let reward = 100 * self.wave;
                        (reward, format!("+{}", reward))
                    } else {
                        (12, "+12".to_string())
                    };
                    self.players[attacker].score += gain;
                    self.award_energy(attacker, gain);
                    self.players[attacker].kills += 1;
                    self.popups.push(ScorePopup {
                        x: fx,
                        y: fy,
                        text,
                        created_at: self.timer,
                    });
                    self.explosions.push(Explosion::new(fx, fy, &v));
                    self.flyers.remove(i);
                    continue;
                }
                self.players[attacker].score += 6;
                self.award_energy(attacker, 6);
                self.popups.push(ScorePopup {
                    x: fx,
                    y: fy,
                    text: "+6".to_string(),
                    created_at: self.timer,
                });
                let attacker_x = self.players[attacker].body.x;
                self.flyers[i].ay = -v.hper(2.0);
                self.flyers[i].ax = if fx > attacker_x {
                    v.wper(0.5)
                } else {
                    -v.wper(0.5)
                };
                // Swing damage, so only a swing pays it - the wall has already
                // taken its share above. What a swing is worth depends on what
                // the player is carrying, and a bullet or a trap carries its
                // own number.
                if !by_field {
                    let attacker_kind = self.players[attacker].attack;
                    let attacker_level = self.players[attacker].attack_level;
                    let strike = self.players[attacker].strike_id;
                    let repeat = source == HitSource::Swing
                        && attacker_kind.once_per_enemy()
                        && self.flyers[i].last_strike == strike
                        && strike != 0;
                    if !repeat {
                        let hit_damage = match source {
                            HitSource::Field => 0.0,
                            HitSource::Swing => attacker_kind.damage(attacker_level),
                            HitSource::Bullet(bi) => {
                                if self.bullets[bi].flying() {
                                    self.bullets[bi].stop();
                                }
                                self.bullets[bi].damage
                            }
                            HitSource::Trap(ti) => self.traps[ti].damage,
                        };
                        self.flyers[i].hp -= hit_damage;
                        self.flyers[i].last_strike = strike;
                    }
                }
                // A teleporter blinks away the first time it is touched, which
                // makes the first exchange with one a surprise rather than a
                // formality. Later hits let it stand and fight. The flying boss
                // never stands still: it goes on every hit.
                let first_hit = !self.flyers[i].hurt_once;
                self.flyers[i].hurt_once = true;
                let blinks = if self.flyers[i].is_boss {
                    true
                } else {
                    first_hit
                        && matches!(self.flyers[i].behavior, FlyerBehavior::Teleporter { .. })
                };
                if blinks {
                    self.teleport_flyer(i);
                }
            }

            for e in 0..self.explosions.len() {
                if self.explosions[e].body.intersects(&self.flyers[i].body) {
                    self.flyers[i].ax = if self.flyers[i].body.x > self.explosions[e].body.x {
                        v.wper(0.5)
                    } else {
                        -v.wper(0.5)
                    };
                }
            }

            for pi in 0..self.players.len() {
                if self.players[pi].dead {
                    continue;
                }
                let dashing = self.players[pi].in_dash_window(timer);
                let p = &self.players[pi];
                if self.flyers[i].body.intersects(&p.body) && !p.field.readiness && !p.field.active
                {
                    // A flyer has no height to shove: its y is recomputed from
                    // its own sine every tick, so anything written there is gone
                    // by the next one. Half a period of phase moves it to the
                    // mirrored point of that arc instead - above the centre line
                    // if it was below, and the other way round.
                    if dashing {
                        self.flyers[i].spawn_offset -= FLYER_DASH_PHASE_SHIFT;
                        continue;
                    }
                    let source = self.flyers[i].body;
                    let ended = self.damage_player(pi, 16.0, &source, Reach::Normal);
                    self.players[pi].ay = -v.hper(2.0);
                    if ended {
                        return true;
                    }
                }
            }

            let phase = (self.timer - self.flyers[i].spawn_offset) as f32 / 50.0;
            self.flyers[i].body.y =
                v.hper(FLYER_ARC_CENTER_PCT) + libm::sinf(phase) * v.hper(FLYER_ARC_SWING_PCT);
            // Flyers are held inside the view rather than inside the world.
            // When the player outruns one it ends up pressed against the
            // trailing edge and is carried along, which reads as pursuit; when
            // the player stands still it goes back to patrolling the screen.
            // Nothing else in a flyer's update ever lowers its speed: `ax` is
            // written on spawn, on being hit and on a blast, and the edges only
            // flip its sign. Without this a shove would not be a shove - it
            // would be the speed the flyer keeps for the rest of its life.
            let cruise = v.wper(FLYER_CRUISE_PCT);
            if self.flyers[i].ax.abs() > cruise {
                let sign = if self.flyers[i].ax > 0.0 { 1.0 } else { -1.0 };
                self.flyers[i].ax =
                    sign * (self.flyers[i].ax.abs() - v.wper(CHASE_ACCEL_PCT)).max(cruise);
            }
            self.flyers[i].body.x += self.flyers[i].ax;
            let (left, right) = (self.camera_x, self.camera_x + v.w - self.flyers[i].body.w);
            if self.flyers[i].body.x < left {
                self.flyers[i].body.x = left;
                self.flyers[i].ax = self.flyers[i].ax.abs();
            } else if self.flyers[i].body.x > right {
                self.flyers[i].body.x = right;
                self.flyers[i].ax = -self.flyers[i].ax.abs();
            }
        }
        false
    }

    /* ---------------- effects ---------------- */

    /// Moves the thrown squares and ages the placed ones.
    ///
    /// Neither deals its damage here: that happens where every other hit does,
    /// through `find_attacker`, so a bullet kill pays score and leaves a blast
    /// exactly like a swing kill does.
    fn update_detached(&mut self) {
        let (left, right) = (self.view_left(), self.view_right());
        for b in self.bullets.iter_mut() {
            if b.flying() {
                b.body.x += b.ax;
                // Gone at the edge of what the player can see. The field runs
                // forever, so without this a miss would travel until the run
                // ended.
                if b.body.x + b.body.w < left || b.body.x > right {
                    b.dead = true;
                }
            } else {
                b.ticks_left -= 1;
                if b.ticks_left <= 0 {
                    b.dead = true;
                }
            }
        }
        self.bullets.retain(|b| !b.dead);

        for t in self.traps.iter_mut() {
            t.ticks_left -= 1;
        }
        self.traps.retain(|t| t.ticks_left > 0);
    }

    fn update_explosions(&mut self) {
        let v = self.viewport;
        for e in self.explosions.iter_mut() {
            e.update(&v);
        }
        self.explosions.retain(|e| !e.finished);
    }

    fn update_projectiles(&mut self) -> bool {
        let v = self.viewport;
        let mut i = self.projectiles.len();
        while i > 0 {
            i -= 1;
            let (vl, vr) = (self.camera_x, self.camera_x + v.w);
            self.projectiles[i].update(&v, vl, vr);
            if !self.projectiles[i].dead {
                for pi in 0..self.players.len() {
                    if self.players[pi].dead {
                        continue;
                    }
                    if self.projectiles[i].body.intersects(&self.players[pi].body) {
                        let source = self.projectiles[i].body;
                        let amount = self.projectiles[i].damage;
                        self.projectiles[i].dead = true;
                        if self.damage_player(pi, amount, &source, Reach::Normal) {
                            return true;
                        }
                        break;
                    }
                }
            }
            if self.projectiles[i].dead {
                self.projectiles.remove(i);
            }
        }
        false
    }
}
