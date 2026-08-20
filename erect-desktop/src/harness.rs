//! Runs the game unattended and saves what it drew.
//!
//! Verifying that something *renders* is the one thing the test suite cannot
//! do: it exercises the core, and a marker that never reaches the screen looks
//! identical from there. Driving a browser by synthetic clicks was the other
//! option and it lost most of them; a native build with a fixed seed, a fixed
//! frame count and a file on disk is repeatable.
//!
//! Off unless `ERECT_HARNESS` is set, so an ordinary run never notices.

use erect_core::dev::DevSetup;
use erect_core::game::Game;
use erect_core::input::InputFrame;
use macroquad::prelude::*;

pub struct Harness {
    /// Frames to draw before saving. Enough to let the wave spawn.
    frames: u32,
    shots: Vec<(u32, String)>,
    /// Saved on the first frame a rolled heavy is being announced. A fixed
    /// frame number cannot catch that: when it arrives depends on how fast the
    /// wave spent its budget.
    on_elite: Option<String>,
    /// Keeps the players standing. Nothing drives them here, so an ordinary run
    /// ends within a wave or two - long before the thing under test arrives.
    keep_alive: bool,
    /// Swings on a fixed rhythm and turns to face whatever is nearest.
    ///
    /// Not a good player - a working one. A wave only spawns as fast as the
    /// field is cleared, so a run that never kills anything stalls at the crowd
    /// limit and the later half of the wave, heavy included, never arrives.
    fight: bool,
    /// Raises a wall every so often, so one can be looked at.
    wall: bool,
}

impl Harness {
    /// `ERECT_HARNESS=wave=7,alive,fight,frames=600,out=/tmp/a.png`
    ///
    /// Flags may be bare or written `=1`; anything unrecognised is ignored, so
    /// a stale spelling is a missing feature rather than a failure to start.
    pub fn from_env(game: &mut Game) -> Option<Self> {
        let spec = std::env::var("ERECT_HARNESS").ok()?;
        let mut dev = DevSetup::default();
        let mut frames = 600;
        let mut shots = Vec::new();
        let mut on_elite = None;
        let mut keep_alive = false;
        let mut fight = false;
        let mut boons = false;
        let mut screen: Option<String> = None;
        let mut wall = erect_core::boon::WallMod::Plain;
        for part in spec.split(',') {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            match key {
                "wave" => dev.wave = value.parse().unwrap_or(1),
                "score" => dev.score = value.parse().unwrap_or(0),
                "frames" => frames = value.parse().unwrap_or(600),
                "out" => shots.push((frames, value.to_string())),
                // A second and later snapshot, so one run can show a thing
                // arriving and the same thing being fought.
                "at" => {
                    if let Some((at, path)) = value.split_once(':') {
                        shots.push((at.parse().unwrap_or(0), path.to_string()));
                    }
                }
                "on_elite" => on_elite = Some(value.to_string()),
                "alive" => keep_alive = true,
                "fight" => fight = true,
                "boons" => boons = true,
                // Show a menu instead of playing, so a layout can be looked at.
                "screen" => screen = Some(value.to_string()),
                "wall" => wall = match value {
                    "pull" => erect_core::boon::WallMod::Pull,
                    "push" => erect_core::boon::WallMod::Push,
                    _ => erect_core::boon::WallMod::Plain,
                },
                _ => {}
            }
        }
        game.dev = dev;
        match screen.as_deref() {
            Some("dev") => game.state = erect_core::game::State::DevMenu,
            Some("title") => game.state = erect_core::game::State::Title,
            _ => game.start_dev_run(),
        }
        shots.sort_by_key(|(at, _)| *at);
        if boons || wall != erect_core::boon::WallMod::Plain {
            for p in game.players.iter_mut() {
                p.boons = erect_core::boon::Boons {
                    double_jump: boons,
                    dash_free: boons,
                    shield: boons,
                    wall,
                };
                // A wall to actually raise, since nothing here earns one.
                p.super_charges = 9;
            }
        }
        Some(Self { frames, shots, on_elite, keep_alive, fight, wall: wall != erect_core::boon::WallMod::Plain })
    }

    /// True when the run is over and the process should stop.
    /// Fills in what a player would be doing.
    pub fn drive(&self, frame: &mut InputFrame, game: &Game, tick: u32) {
        // Jump, then slam: the wall needs the player off the floor.
        if self.wall {
            if let Some(me) = game.players.first() {
                frame.players[0].jump = me.grounded && tick % 90 == 0;
                frame.players[0].slam = !me.grounded && tick % 90 == 6;
            }
        }
        if !self.fight {
            return;
        }
        let Some(me) = game.players.first() else {
            return;
        };
        let nearest = game
            .zombies
            .iter()
            .map(|z| z.body.x)
            .chain(game.flyers.iter().map(|f| f.body.x))
            .min_by(|a, b| {
                (a - me.body.x)
                    .abs()
                    .partial_cmp(&(b - me.body.x).abs())
                    .unwrap()
            });
        let intent = &mut frame.players[0];
        if let Some(x) = nearest {
            let away = x - me.body.x;
            intent.right = away > 0.0;
            intent.left = away < 0.0;
        }
        // A swing every few ticks: held down, most kinds would only swing once.
        intent.attack = tick % 12 == 0;
        intent.attack_held = true;
    }

    /// Before the tick, so the players never reach a frame dead.
    pub fn before_tick(&self, game: &mut Game) {
        if self.wall {
            for p in game.players.iter_mut() {
                p.super_charges = p.super_charges.max(1);
            }
        }
        if self.keep_alive {
            for p in game.players.iter_mut() {
                p.hp = p.hpmax;
                p.dead = false;
            }
        }
    }

    pub fn after_frame(&mut self, frame: u32, game: &Game) -> bool {
        if let Some(path) = self.on_elite.as_ref() {
            if game.elite_notice().is_some() {
                get_screen_data().export_png(path);
                println!("harness: frame {frame} (heavy announced) -> {path}");
                self.on_elite = None;
            }
        }
        while let Some((at, path)) = self.shots.first() {
            if frame < *at {
                break;
            }
            // Straight off the framebuffer, so it is exactly what a player
            // would be looking at - pad, letterboxing and all.
            get_screen_data().export_png(path);
            println!("harness: frame {frame} -> {path}");
            self.shots.remove(0);
        }
        // The frame cap ends the run whatever happened. An event that never
        // fires is a result to report, not a reason to sit there drawing.
        if frame < self.frames {
            return false;
        }
        if let Some(path) = self.on_elite.take() {
            println!("harness: no heavy was ever announced, {path} not written");
        }
        true
    }
}
