//! Browser audio: the same pack, played by the browser instead of by us.
//!
//! The desktop carries its own mixer because six music layers have to stay
//! sample-locked and no per-source playback API can promise that. Web Audio
//! can: sources scheduled for one instant begin together and stay together, so
//! handing it six looped buffers and six gain nodes gets the lock for free.
//! What is left for this module is the part that was never about mixing -
//! deciding what each layer's gain should be - and that is the core's
//! [`GainEngine`], shared with every other platform.
//!
//! The files come off the network rather than off a disk, which is the one
//! thing that genuinely differs: nine of them, nine megabytes, and nothing to
//! listen to until they are all decoded. Hence [`Loader`], and the screen it
//! is drawn behind.
//!
//! One pack a session, too. A desktop holds all three in memory and swaps on
//! reaching the menu; here that would mean fetching another nine megabytes
//! mid-game for a change of soundtrack nobody asked for, so the roll is made
//! once when the page loads and reloading is what changes it.

use erect_audio::packs::{self, Pack};
use erect_audio::GainEngine;
use erect_core::audio::{AudioEvent, MusicState};
use macroquad::prelude::get_time;

// Written by `web/erect_web.js`. See that file for why the browser's own audio
// API is used directly instead of macroquad's `audio` feature.
#[link(wasm_import_module = "env")]
extern "C" {
    /// Hands a file to the browser to decode. Answers an id, never zero.
    fn erect_audio_decode(ptr: *const u8, len: u32) -> u32;
    /// 0 still decoding, 1 ready, 2 gave up.
    fn erect_audio_ready(id: u32) -> u32;
    /// Starts every music layer at one shared instant.
    fn erect_audio_music_start(ids: *const u32, count: u32);
    fn erect_audio_music_gain(slot: u32, value: f32);
    fn erect_audio_sfx_play(id: u32, volume: f32);
}

/// A decoded sound living on the browser's side.
type Clip = u32;

/// Where a built pack's desktop encoding sits, relative to the served page.
/// `tools/web.sh` copies the tree in beside the wasm.
fn path(dir: &str, id: &str) -> String {
    format!("packs/{dir}/desktop/{id}.flac")
}

/// The three effects, in the order the pack lists them.
fn sfx_index(event: AudioEvent) -> usize {
    match event {
        AudioEvent::Hit => 0,
        AudioEvent::Jump => 1,
        AudioEvent::Slam => 2,
    }
}

/// A percentage from the settings menu as a factor.
fn as_factor(percent: u32) -> f32 {
    (percent as f32 / 100.0).clamp(0.0, 1.0)
}

/// Loads one pack, a file per step, so the caller can draw between them.
///
/// Stepwise rather than all at once on purpose: a browser gives no progress on
/// a decode, and a page that goes still for several seconds with nothing on it
/// looks broken. One file per frame is honest about what is happening and costs
/// nothing - the fetches dominate either way.
pub struct Loader {
    pack: &'static Pack,
    clips: Vec<Clip>,
    step: usize,
    failed: Option<String>,
    /// The file handed to the browser and not yet decoded.
    ///
    /// Kept here rather than waited on inside a step, so the only thing that
    /// ever spans frames is the fetch. A wait that spans frames without drawing
    /// leaves the loading screen stale - and macroquad, whose font atlas is
    /// rebuilt per frame, on a texture it has already thrown away.
    pending: Option<Clip>,
}

impl Loader {
    pub fn new(seed: u64) -> Self {
        Self {
            pack: packs::choose(seed),
            clips: Vec::new(),
            step: 0,
            failed: None,
            pending: None,
        }
    }

    /// Files in this pack, music then effects.
    pub fn total(&self) -> usize {
        packs::LAYER_IDS.len() + self.pack.sfx.len()
    }

    pub fn done(&self) -> usize {
        self.step
    }

    /// What is being fetched right now, for the screen to name.
    pub fn current(&self) -> &'static str {
        let n = packs::LAYER_IDS.len();
        if self.step < n {
            packs::LAYER_IDS[self.step]
        } else {
            self.pack
                .sfx
                .get(self.step - n)
                .map(|(id, _, _, _)| *id)
                .unwrap_or("")
        }
    }

    /// Moves the load along by as little as it can.
    ///
    /// Either checks the file the browser is decoding, or starts the next one.
    /// Never more, so the caller gets a frame to draw between every visible
    /// change.
    pub async fn pump(&mut self) {
        if self.failed.is_some() || self.step >= self.total() {
            return;
        }
        if let Some(clip) = self.pending {
            match unsafe { erect_audio_ready(clip) } {
                1 => {
                    self.clips.push(clip);
                    self.pending = None;
                    self.step += 1;
                }
                // A browser that cannot decode FLAC leaves the game silent
                // rather than stopped; there is nothing the player can do about
                // it and nothing about the game needs sound to work.
                2 => self.failed = Some(format!("{}: could not be decoded", self.current())),
                _ => {}
            }
            return;
        }
        let file = path(self.pack.dir, self.current());
        match macroquad::file::load_file(&file).await {
            Ok(bytes) => {
                self.pending =
                    Some(unsafe { erect_audio_decode(bytes.as_ptr(), bytes.len() as u32) });
            }
            Err(why) => self.failed = Some(format!("{file}: {why}")),
        }
    }

    /// True once there is nothing left to do, finished or given up.
    pub fn settled(&self) -> bool {
        self.failed.is_some() || self.step >= self.total()
    }

    /// How many files are actually in hand, for a failure to be able to say.
    pub fn loaded(&self) -> usize {
        self.clips.len()
    }

    pub fn problem(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// The finished thing, or nothing if a file never arrived.
    pub fn finish(self) -> Option<Sound> {
        if self.failed.is_some() || self.clips.len() != self.total() {
            return None;
        }
        let n = packs::LAYER_IDS.len();
        let mut clips = self.clips;
        let sfx = clips.split_off(n);
        Some(Sound {
            layers: clips,
            sfx,
            gains: GainEngine::new(packs::layers(self.pack.desktop_gains_db)),
            pack: self.pack,
            music_volume: 1.0,
            sfx_volume: 1.0,
            last_fired: [f64::NEG_INFINITY; 3],
            playing: false,
        })
    }
}

pub struct Sound {
    layers: Vec<Clip>,
    sfx: Vec<Clip>,
    gains: GainEngine,
    pack: &'static Pack,
    music_volume: f32,
    sfx_volume: f32,
    /// When each effect last played, for the pack's own minimum spacing. A
    /// swing every tick is six of the same sound inside a tenth of a second,
    /// which is a buzz rather than six hits.
    last_fired: [f64; 3],
    playing: bool,
}

impl Sound {
    /// Starts all six layers at once, silent.
    ///
    /// All at once is the whole point: sources scheduled in one turn land in
    /// the same render quantum, and from there they stay together for as long
    /// as they loop. Started one by one as they became audible, they would each
    /// begin wherever the music happened to be.
    fn start(&mut self) {
        unsafe { erect_audio_music_start(self.layers.as_ptr(), self.layers.len() as u32) };
        self.playing = true;
    }

    /// Per frame: advance the fades and hand the browser the six numbers.
    pub fn update(&mut self, state: MusicState, dt_ms: f32) {
        if !self.playing {
            self.start();
        }
        self.gains.update(&state, dt_ms);
        for i in 0..self.layers.len() {
            unsafe { erect_audio_music_gain(i as u32, self.gains.gain(i) * self.music_volume) };
        }
    }

    pub fn fire(&mut self, event: AudioEvent) {
        let i = sfx_index(event);
        let Some(clip) = self.sfx.get(i) else { return };
        let (_, _, min_interval_ms, _) = self.pack.sfx[i];
        let now = get_time();
        if (now - self.last_fired[i]) * 1000.0 < min_interval_ms as f64 {
            return;
        }
        self.last_fired[i] = now;
        unsafe { erect_audio_sfx_play(*clip, self.sfx_volume) };
    }

    pub fn set_volumes(&mut self, music: u32, sfx: u32) {
        self.music_volume = as_factor(music);
        self.sfx_volume = as_factor(sfx);
    }

}
